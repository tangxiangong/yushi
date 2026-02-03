use crate::{
    Error, Result,
    state::{ChunkState, DownloadState, QueueState, current_timestamp},
    types::{
        ChecksumType, CompletionCallback, Config, DownloaderEvent, ProgressEvent, Task, TaskEvent,
        TaskPriority, TaskStatus, VerificationEvent,
    },
    utils::{SpeedCalculator, SpeedLimiter, auto_rename, verify_file},
};
use fs_err::tokio as fs;
use futures::StreamExt;
use reqwest::{
    Client, Proxy,
    header::{CONTENT_LENGTH, RANGE, USER_AGENT},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::{RwLock, Semaphore, mpsc},
    task::JoinHandle,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct YuShi {
    client: Client,
    config: Config,
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    active_downloads: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
    max_concurrent_tasks: usize,
    queue_state_path: PathBuf,
    queue_event_tx: mpsc::Sender<DownloaderEvent>,
    on_complete: Option<CompletionCallback>,
}

impl std::fmt::Debug for YuShi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YuShi")
            .field("config", &self.config)
            .field("max_concurrent_tasks", &self.max_concurrent_tasks)
            .field("queue_state_path", &self.queue_state_path)
            .field("has_on_complete", &self.on_complete.is_some())
            .finish()
    }
}

impl YuShi {
    /// 创建新的下载器实例
    ///
    /// # 参数
    /// * `max_concurrent_downloads` - 每个任务的最大并发下载连接数
    /// * `max_concurrent_tasks` - 队列中同时运行的最大任务数
    /// * `queue_state_path` - 队列状态持久化文件路径
    ///
    /// # 返回
    /// 返回下载器实例和队列事件接收器
    pub fn new(
        max_concurrent_downloads: usize,
        max_concurrent_tasks: usize,
        queue_state_path: PathBuf,
    ) -> (Self, mpsc::Receiver<DownloaderEvent>) {
        let config = Config {
            max_concurrent: max_concurrent_downloads,
            ..Default::default()
        };
        Self::with_config(config, max_concurrent_tasks, queue_state_path)
    }

    /// 使用自定义配置创建下载器
    ///
    /// # 参数
    /// * `config` - 下载配置
    /// * `max_concurrent_tasks` - 队列中同时运行的最大任务数
    /// * `queue_state_path` - 队列状态持久化文件路径
    ///
    /// # 返回
    /// 返回下载器实例和队列事件接收器
    pub fn with_config(
        config: Config,
        max_concurrent_tasks: usize,
        queue_state_path: PathBuf,
    ) -> (Self, mpsc::Receiver<DownloaderEvent>) {
        let (event_tx, event_rx) = mpsc::channel(1024);

        let mut builder = Client::builder()
            .tcp_keepalive(Duration::from_secs(60))
            .timeout(Duration::from_secs(config.timeout));

        if let Some(proxy_url) = &config.proxy
            && let Ok(proxy) = Proxy::all(proxy_url)
        {
            builder = builder.proxy(proxy);
        }

        let client = builder.build().unwrap();

        let downloader = Self {
            client,
            config,
            tasks: Arc::new(RwLock::new(HashMap::new())),
            active_downloads: Arc::new(RwLock::new(HashMap::new())),
            max_concurrent_tasks,
            queue_state_path,
            queue_event_tx: event_tx,
            on_complete: None,
        };

        (downloader, event_rx)
    }

    /// 设置下载完成回调
    pub fn set_on_complete<F, Fut>(&mut self, callback: F)
    where
        F: Fn(String, std::result::Result<(), String>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.on_complete = Some(Arc::new(move |task_id, result| {
            Box::pin(callback(task_id, result))
        }));
    }

    /// 简单下载文件（单文件下载的便捷方法）
    ///
    /// # 参数
    /// * `url` - 下载 URL
    /// * `dest` - 目标文件路径
    /// * `event_tx` - 进度事件发送器（可选）
    pub async fn download(
        &self,
        url: &str,
        dest: &str,
        event_tx: Option<mpsc::Sender<ProgressEvent>>,
    ) -> Result<()> {
        // 添加任务到队列
        let task_id = self.add_task(url.to_string(), PathBuf::from(dest)).await?;

        // 等待任务完成
        loop {
            let task = self.get_task(&task_id).await;
            if let Some(task) = task {
                match task.status {
                    TaskStatus::Completed => return Ok(()),
                    TaskStatus::Failed => {
                        return Err(Error::TaskFailed(
                            task.error.unwrap_or_else(|| "Unknown error".to_string()),
                        ));
                    }
                    TaskStatus::Cancelled => {
                        return Err(Error::TaskCancelled);
                    }
                    _ => {
                        // 如果提供了进度事件发送器，发送进度更新
                        if let Some(tx) = &event_tx {
                            if task.total_size > 0 {
                                tx.send(ProgressEvent::ChunkDownloading {
                                    chunk_index: 0,
                                    delta: 0, // 这里不发送增量，只是为了兼容
                                })
                                .await?;
                            } else {
                                tx.send(ProgressEvent::StreamDownloading {
                                    downloaded: task.downloaded,
                                })
                                .await?;
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            } else {
                return Err(Error::TaskNotFound);
            }
        }
    }

    /// 内部下载方法（由队列任务调用）
    ///
    /// # 参数
    /// * `url` - 下载 URL
    /// * `dest` - 目标文件路径
    /// * `event_tx` - 进度事件发送器
    async fn download_internal(
        &self,
        url: &str,
        dest: &str,
        event_tx: mpsc::Sender<ProgressEvent>,
    ) -> Result<()> {
        let dest_path = PathBuf::from(dest);
        let state_path = dest_path.with_extension("json");

        let state = self
            .get_or_create_state(url, &dest_path, &state_path)
            .await?;
        let state = Arc::new(RwLock::new(state));

        let (total_size, is_streaming) = {
            let s = state.read().await;
            (s.total_size, s.is_streaming)
        };

        event_tx
            .send(ProgressEvent::Initialized {
                task_id: "internal".to_string(),
                total_size,
            })
            .await?;

        if is_streaming {
            // 流式下载
            self.download_streaming(url, &dest_path, event_tx).await
        } else {
            // 分块下载
            self.download_chunked(state, &dest_path, &state_path, event_tx)
                .await
        }
    }

    /// 流式下载（不需要 Content-Length）
    async fn download_streaming(
        &self,
        url: &str,
        dest: &std::path::PathBuf,
        event_tx: mpsc::Sender<ProgressEvent>,
    ) -> Result<()> {
        let mut request = self.client.get(url);

        // 添加自定义头
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        // 添加 User-Agent
        if let Some(ua) = &self.config.user_agent {
            request = request.header(USER_AGENT, ua);
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(Error::HttpError(response.status().to_string()));
        }

        let mut file = fs::File::create(dest).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0u64;
        let speed_limiter = self
            .config
            .speed_limit
            .map(|limit| Arc::new(RwLock::new(SpeedLimiter::new(limit))));

        while let Some(item) = stream.next().await {
            let chunk_data = item.map_err(|e| Error::StreamError(e.to_string()))?;
            file.write_all(&chunk_data).await?;

            let len = chunk_data.len() as u64;
            downloaded += len;

            if let Some(speed_limiter) = &speed_limiter {
                speed_limiter.write().await.wait(len).await;
            }

            let _ = event_tx
                .send(ProgressEvent::StreamDownloading { downloaded })
                .await;
        }

        file.flush().await?;
        event_tx
            .send(ProgressEvent::Finished {
                task_id: "internal".to_string(),
            })
            .await?;
        Ok(())
    }

    /// 分块下载（需要 Content-Length）
    async fn download_chunked(
        &self,
        state: Arc<tokio::sync::RwLock<DownloadState>>,
        dest_path: &Path,
        state_path: &Path,
        event_tx: mpsc::Sender<ProgressEvent>,
    ) -> Result<()> {
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent));
        let speed_limiter = self
            .config
            .speed_limit
            .map(|limit| Arc::new(RwLock::new(SpeedLimiter::new(limit))));
        let mut workers = Vec::new();

        let (chunks_count, url) = {
            let s = state.read().await;
            (s.chunks.len(), s.url.clone())
        };

        for i in 0..chunks_count {
            let permit = semaphore.clone().acquire_owned().await?;
            let state_c = Arc::clone(&state);
            let client_c = self.client.clone();
            let url_c = url.clone();
            let dest_c = dest_path.to_path_buf();
            let state_file_c = state_path.to_path_buf();
            let tx_c = event_tx.clone();
            let speed_limiter_c = speed_limiter.clone();
            let headers = self.config.headers.clone();
            let user_agent = self.config.user_agent.clone();

            workers.push(tokio::spawn(async move {
                let res = Self::download_chunk(
                    i,
                    client_c,
                    &url_c,
                    &dest_c,
                    &state_file_c,
                    state_c,
                    tx_c,
                    speed_limiter_c,
                    headers,
                    user_agent,
                )
                .await;
                drop(permit);
                res
            }));
        }

        for worker in workers {
            worker.await??;
        }

        fs::remove_file(state_path).await?;
        event_tx
            .send(ProgressEvent::Finished {
                task_id: "internal".to_string(),
            })
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    /// 下载单个分块
    async fn download_chunk(
        index: usize,
        client: reqwest::Client,
        url: &str,
        dest: &Path,
        state_file: &Path,
        state_lock: Arc<tokio::sync::RwLock<DownloadState>>,
        tx: mpsc::Sender<ProgressEvent>,
        speed_limiter: Option<Arc<RwLock<SpeedLimiter>>>,
        headers: std::collections::HashMap<String, String>,
        user_agent: Option<String>,
    ) -> Result<()> {
        let (start_pos, end_pos) = {
            let s = state_lock.read().await;
            let chunk = &s.chunks[index];
            if chunk.is_finished {
                return Ok(());
            }
            (chunk.current, chunk.end)
        };

        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 5;

        loop {
            let mut request = client
                .get(url)
                .header(RANGE, format!("bytes={}-{}", start_pos, end_pos));

            // 添加自定义头
            for (key, value) in &headers {
                request = request.header(key, value);
            }

            // 添加 User-Agent
            if let Some(ua) = &user_agent {
                request = request.header(USER_AGENT, ua);
            }

            let res = request.send().await;

            match res {
                Ok(resp) if resp.status().is_success() => {
                    let mut file = fs::OpenOptions::new().write(true).open(&dest).await?;
                    file.seek(SeekFrom::Start(start_pos)).await?;

                    let mut stream = resp.bytes_stream();
                    let mut current_idx = start_pos;

                    while let Some(item) = stream.next().await {
                        let chunk_data = item.map_err(|e| Error::StreamError(e.to_string()))?;
                        file.write_all(&chunk_data).await?;

                        let len = chunk_data.len() as u64;
                        current_idx += len;

                        if let Some(speed_limiter) = &speed_limiter {
                            speed_limiter.write().await.wait(len).await;
                        }

                        // 更新内存状态
                        {
                            let mut s = state_lock.write().await;
                            s.chunks[index].current = current_idx;
                        }

                        let _ = tx
                            .send(ProgressEvent::ChunkDownloading {
                                chunk_index: index,
                                delta: len,
                            })
                            .await;

                        // 保存状态
                        let state = state_lock.read().await;
                        state.save(state_file).await?;
                    }

                    let mut s = state_lock.write().await;
                    s.chunks[index].is_finished = true;
                    return Ok(());
                }
                _ => {
                    retry_count += 1;
                    if retry_count > MAX_RETRIES {
                        return Err(Error::HttpError(format!(
                            "Chunk {} failed after {} retries",
                            index, MAX_RETRIES
                        )));
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    /// 获取或创建下载状态
    async fn get_or_create_state(
        &self,
        url: &str,
        dest: &Path,
        state_path: &Path,
    ) -> Result<DownloadState> {
        // 尝试加载已有状态
        if let Some(state) = DownloadState::load(state_path).await?
            && state.url == url
        {
            return Ok(state);
        }

        // 检查服务器是否支持 Range 请求和 Content-Length
        let res = self.client.head(url).send().await?;
        let total_size_opt = res
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok()?.parse::<u64>().ok());

        let supports_range = res
            .headers()
            .get("accept-ranges")
            .map(|v| v.to_str().unwrap_or("").contains("bytes"))
            .unwrap_or(false);

        let use_streaming = total_size_opt.is_none() || !supports_range;

        if use_streaming {
            // 流式下载模式
            return Ok(DownloadState {
                url: url.to_string(),
                total_size: total_size_opt,
                chunks: Vec::new(),
                is_streaming: true,
            });
        }

        // 分块下载模式
        let total_size = total_size_opt.unwrap(); // 已经检查过存在

        let file = fs::File::create(dest).await?;
        file.set_len(total_size).await?;

        let mut chunks = Vec::new();
        let mut curr = 0;
        let mut idx = 0;
        while curr < total_size {
            let end = (curr + self.config.chunk_size - 1).min(total_size - 1);
            chunks.push(ChunkState {
                index: idx,
                start: curr,
                end,
                current: curr,
                is_finished: false,
            });
            curr += self.config.chunk_size;
            idx += 1;
        }

        let state = DownloadState {
            url: url.to_string(),
            total_size: Some(total_size),
            chunks,
            is_streaming: false,
        };
        state.save(state_path).await?;
        Ok(state)
    }

    // ==================== 队列管理方法 ====================

    /// 从持久化状态加载队列
    pub async fn load_queue_from_state(&self) -> Result<()> {
        if let Some(state) = QueueState::load(&self.queue_state_path).await? {
            let mut tasks = self.tasks.write().await;
            for task in state.tasks {
                tasks.insert(task.id.clone(), task);
            }
        }
        Ok(())
    }

    /// 保存队列状态
    async fn save_queue_state(&self) -> Result<()> {
        let tasks = self.tasks.read().await;
        let task_list: Vec<Task> = tasks.values().cloned().collect();

        let state = QueueState {
            version: "1.0".to_string(),
            tasks: task_list,
            created_at: current_timestamp(),
            updated_at: current_timestamp(),
        };
        state.save(&self.queue_state_path).await?;
        Ok(())
    }

    /// 添加下载任务到队列
    ///
    /// # 参数
    /// * `url` - 下载 URL
    /// * `dest` - 目标文件路径
    ///
    /// # 返回
    /// 返回任务 ID
    pub async fn add_task(&self, url: String, dest: PathBuf) -> Result<String> {
        self.add_task_with_options(url, dest, TaskPriority::Normal, None, false)
            .await
    }

    /// 添加下载任务到队列（带选项）
    ///
    /// # 参数
    /// * `url` - 下载 URL
    /// * `dest` - 目标文件路径
    /// * `priority` - 任务优先级
    /// * `checksum` - 文件校验（可选）
    /// * `auto_rename_on_conflict` - 是否自动重命名冲突文件
    ///
    /// # 返回
    /// 返回任务 ID
    pub async fn add_task_with_options(
        &self,
        url: String,
        mut dest: PathBuf,
        priority: TaskPriority,
        checksum: Option<ChecksumType>,
        auto_rename_on_conflict: bool,
    ) -> Result<String> {
        // 自动重命名
        if auto_rename_on_conflict && dest.exists() {
            dest = auto_rename(&dest);
        }

        let task_id = Uuid::new_v4().to_string();

        let task = Task {
            id: task_id.clone(),
            url,
            dest,
            status: TaskStatus::Pending,
            total_size: 0,
            downloaded: 0,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            error: None,
            priority,
            speed: 0,
            eta: None,
            headers: HashMap::new(),
            checksum,
        };

        {
            let mut tasks = self.tasks.write().await;
            tasks.insert(task_id.clone(), task);
        }

        self.save_queue_state().await?;
        let _ = self
            .queue_event_tx
            .send(DownloaderEvent::Task(TaskEvent::Added {
                task_id: task_id.clone(),
            }))
            .await;

        // 尝试启动任务
        self.process_queue().await?;

        Ok(task_id)
    }

    /// 处理队列，启动待处理的任务（按优先级排序）
    async fn process_queue(&self) -> Result<()> {
        let active_count = self.active_downloads.read().await.len();
        if active_count >= self.max_concurrent_tasks {
            return Ok(());
        }

        let mut pending_tasks: Vec<(String, TaskPriority)> = {
            let tasks = self.tasks.read().await;
            tasks
                .values()
                .filter(|t| t.status == TaskStatus::Pending)
                .map(|t| (t.id.clone(), t.priority))
                .collect()
        };

        // 按优先级排序（高优先级在前）
        pending_tasks.sort_by(|a, b| b.1.cmp(&a.1));

        for (task_id, _) in pending_tasks
            .iter()
            .take(self.max_concurrent_tasks - active_count)
        {
            self.start_queue_task(task_id).await?;
        }

        Ok(())
    }

    /// 启动单个队列任务
    async fn start_queue_task(&self, task_id: &str) -> Result<()> {
        let task = {
            let mut tasks = self.tasks.write().await;
            let task = tasks.get_mut(task_id).ok_or(Error::TaskNotFound)?;

            if task.status != TaskStatus::Pending && task.status != TaskStatus::Paused {
                return Ok(());
            }

            task.status = TaskStatus::Downloading;
            task.clone()
        };

        self.save_queue_state().await?;
        let _ = self
            .queue_event_tx
            .send(DownloaderEvent::Task(TaskEvent::Started {
                task_id: task_id.to_string(),
            }))
            .await;

        let downloader = self.clone();
        let tasks = Arc::clone(&self.tasks);
        let active_downloads = Arc::clone(&self.active_downloads);
        let queue_event_tx = self.queue_event_tx.clone();
        let task_id_owned = task_id.to_string();
        let queue_state_path = self.queue_state_path.clone();
        let on_complete = self.on_complete.clone();

        let handle = tokio::spawn(async move {
            let (tx, mut rx) = mpsc::channel(1024);
            let task_id_clone = task_id_owned.clone();
            let queue_event_tx_clone = queue_event_tx.clone();
            let tasks_clone = Arc::clone(&tasks);

            // 进度监听器
            tokio::spawn(async move {
                let mut total = 0u64;
                let mut downloaded = 0u64;
                let mut speed_calc = SpeedCalculator::new();

                while let Some(event) = rx.recv().await {
                    match event {
                        ProgressEvent::Initialized { total_size, .. } => {
                            if let Some(size) = total_size {
                                total = size;
                            }
                            let mut tasks = tasks_clone.write().await;
                            if let Some(task) = tasks.get_mut(&task_id_clone) {
                                task.total_size = total_size.unwrap_or(0);
                            }
                        }
                        ProgressEvent::ChunkDownloading { delta, .. } => {
                            downloaded += delta;

                            // 更新速度统计
                            let speed = speed_calc.update(downloaded);
                            let eta = if total > 0 {
                                speed_calc.calculate_eta(downloaded, total)
                            } else {
                                None
                            };

                            let mut tasks = tasks_clone.write().await;
                            if let Some(task) = tasks.get_mut(&task_id_clone) {
                                task.downloaded = downloaded;
                                task.speed = speed;
                                task.eta = eta;
                            }

                            let _ = queue_event_tx_clone
                                .send(DownloaderEvent::Progress(ProgressEvent::Updated {
                                    task_id: task_id_clone.clone(),
                                    downloaded,
                                    total,
                                    speed,
                                    eta,
                                }))
                                .await;
                        }
                        ProgressEvent::StreamDownloading {
                            downloaded: stream_downloaded,
                        } => {
                            downloaded = stream_downloaded;

                            // 更新速度统计
                            let speed = speed_calc.update(downloaded);

                            let mut tasks = tasks_clone.write().await;
                            if let Some(task) = tasks.get_mut(&task_id_clone) {
                                task.downloaded = downloaded;
                                task.speed = speed;
                                task.eta = None; // 流式下载无法预估剩余时间
                            }

                            let _ = queue_event_tx_clone
                                .send(DownloaderEvent::Progress(ProgressEvent::Updated {
                                    task_id: task_id_clone.clone(),
                                    downloaded,
                                    total: 0, // 流式下载时 total 为 0
                                    speed,
                                    eta: None,
                                }))
                                .await;
                        }
                        ProgressEvent::Finished { .. } => {}
                        ProgressEvent::Failed { .. } => {}
                        ProgressEvent::Updated { .. } => {}
                        ProgressEvent::ChunkProgress { .. } => {}
                        ProgressEvent::StreamProgress { .. } => {}
                    }
                }
            });

            // 执行下载
            let result = downloader
                .download_internal(&task.url, task.dest.to_str().unwrap(), tx)
                .await;

            // 文件校验
            let checksum = task.checksum.clone();
            let dest_path = task.dest.clone();
            let verify_result = if result.is_ok() {
                if let Some(checksum_value) = checksum {
                    let _ = queue_event_tx
                        .send(DownloaderEvent::Verification(VerificationEvent::Started {
                            task_id: task_id_owned.clone(),
                        }))
                        .await;

                    match verify_file(&dest_path, &checksum_value).await {
                        Ok(success) => {
                            let _ = queue_event_tx
                                .send(DownloaderEvent::Verification(
                                    VerificationEvent::Completed {
                                        task_id: task_id_owned.clone(),
                                        success,
                                    },
                                ))
                                .await;
                            if success {
                                Ok(())
                            } else {
                                Err(Error::ChecksumVerificationFailed)
                            }
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    result
                }
            } else {
                result
            };

            // 更新任务状态并调用回调
            let callback_result = match &verify_result {
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            };

            let mut tasks = tasks.write().await;
            if let Some(task) = tasks.get_mut(&task_id_owned) {
                match verify_result {
                    Ok(_) => {
                        task.status = TaskStatus::Completed;
                        let _ = queue_event_tx
                            .send(DownloaderEvent::Task(TaskEvent::Completed {
                                task_id: task_id_owned.clone(),
                            }))
                            .await;
                    }
                    Err(e) => {
                        task.status = TaskStatus::Failed;
                        task.error = Some(e.to_string());
                        let _ = queue_event_tx
                            .send(DownloaderEvent::Task(TaskEvent::Failed {
                                task_id: task_id_owned.clone(),
                                error: e.to_string(),
                            }))
                            .await;
                    }
                }
            }

            // 保存状态
            let task_list: Vec<Task> = tasks.values().cloned().collect();
            let state = QueueState {
                version: "1.0".to_string(),
                tasks: task_list,
                created_at: current_timestamp(),
                updated_at: current_timestamp(),
            };
            if let Ok(data) = serde_json::to_string_pretty(&state) {
                let _ = fs::write(&queue_state_path, data).await;
            }

            // 调用完成回调
            if let Some(callback) = on_complete {
                callback(task_id_owned.clone(), callback_result).await;
            }

            // 从活动下载中移除
            active_downloads.write().await.remove(&task_id_owned);
        });

        self.active_downloads
            .write()
            .await
            .insert(task_id.to_string(), handle);

        Ok(())
    }

    /// 暂停任务
    pub async fn pause_task(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        let task = tasks.get_mut(task_id).ok_or(Error::TaskNotFound)?;

        if task.status == TaskStatus::Downloading {
            // 取消当前的下载任务
            let mut active = self.active_downloads.write().await;
            if let Some(handle) = active.remove(task_id) {
                handle.abort();
            }

            task.status = TaskStatus::Paused;
            drop(tasks);
            drop(active);

            self.save_queue_state().await?;
            let _ = self
                .queue_event_tx
                .send(DownloaderEvent::Task(TaskEvent::Paused {
                    task_id: task_id.to_string(),
                }))
                .await;
        }

        Ok(())
    }

    /// 恢复任务
    pub async fn resume_task(&self, task_id: &str) -> Result<()> {
        {
            let mut tasks = self.tasks.write().await;
            let task = tasks.get_mut(task_id).ok_or(Error::TaskNotFound)?;

            if task.status == TaskStatus::Paused {
                task.status = TaskStatus::Pending;
                drop(tasks);

                self.save_queue_state().await?;
                let _ = self
                    .queue_event_tx
                    .send(DownloaderEvent::Task(TaskEvent::Resumed {
                        task_id: task_id.to_string(),
                    }))
                    .await;
            }
        }

        self.process_queue().await?;
        Ok(())
    }

    /// 取消任务
    pub async fn cancel_task(&self, task_id: &str) -> Result<()> {
        // 如果正在下载，先停止
        let mut active = self.active_downloads.write().await;
        if let Some(handle) = active.remove(task_id) {
            handle.abort();
        }
        drop(active);

        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = TaskStatus::Cancelled;

            // 删除下载文件和状态文件
            let _ = fs::remove_file(&task.dest).await;
            let state_path = task.dest.with_extension("json");
            let _ = fs::remove_file(state_path).await;
        }
        drop(tasks);

        self.save_queue_state().await?;
        let _ = self
            .queue_event_tx
            .send(DownloaderEvent::Task(TaskEvent::Cancelled {
                task_id: task_id.to_string(),
            }))
            .await;

        // 处理队列中的下一个任务
        self.process_queue().await?;

        Ok(())
    }

    /// 移除已完成或已取消的任务
    pub async fn remove_task(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get(task_id)
            && (task.status == TaskStatus::Completed
                || task.status == TaskStatus::Cancelled
                || task.status == TaskStatus::Failed)
        {
            tasks.remove(task_id);
            drop(tasks);
            self.save_queue_state().await?;
            return Ok(());
        }
        Err(Error::CannotRemoveTaskInCurrentStatus)
    }

    /// 获取所有任务
    pub async fn get_all_tasks(&self) -> Vec<Task> {
        let tasks = self.tasks.read().await;
        tasks.values().cloned().collect()
    }

    /// 获取单个任务
    pub async fn get_task(&self, task_id: &str) -> Option<Task> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).cloned()
    }

    /// 清空所有已完成的任务
    pub async fn clear_completed(&self) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        tasks.retain(|_, task| task.status != TaskStatus::Completed);
        drop(tasks);
        self.save_queue_state().await?;
        Ok(())
    }
}
