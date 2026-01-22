use crate::{
    state::{ChunkState, DownloadState},
    types::{DownloadConfig, ProgressEvent},
    utils::SpeedLimiter,
};
use anyhow::{Result, anyhow};
use fs_err::tokio as fs;
use futures::StreamExt;
use reqwest::header::{CONTENT_LENGTH, RANGE, USER_AGENT};
use std::{path::Path, sync::Arc};
use tokio::{
    io::{AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::{RwLock as TokioRwLock, Semaphore, mpsc},
};

/// 单文件下载器
pub struct YuShi {
    client: reqwest::Client,
    config: DownloadConfig,
}

impl YuShi {
    /// 创建新的下载器实例
    ///
    /// # 参数
    /// * `max_concurrent` - 最大并发连接数（分块下载）
    pub fn new(max_concurrent: usize) -> Self {
        let config = DownloadConfig {
            max_concurrent,
            ..Default::default()
        };
        Self::with_config(config)
    }

    /// 使用自定义配置创建下载器
    pub fn with_config(config: DownloadConfig) -> Self {
        let mut builder = reqwest::Client::builder()
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(config.timeout));

        // 设置代理
        if let Some(proxy_url) = &config.proxy
            && let Ok(proxy) = reqwest::Proxy::all(proxy_url)
        {
            builder = builder.proxy(proxy);
        }

        let client = builder.build().unwrap();

        Self { client, config }
    }

    /// 下载文件
    ///
    /// # 参数
    /// * `url` - 下载 URL
    /// * `dest` - 目标文件路径
    /// * `event_tx` - 进度事件发送器
    pub async fn download(
        &self,
        url: &str,
        dest: &str,
        event_tx: mpsc::Sender<ProgressEvent>,
    ) -> Result<()> {
        let dest_path = std::path::PathBuf::from(dest);
        let state_path = dest_path.with_extension("json");

        let state = self
            .get_or_create_state(url, &dest_path, &state_path)
            .await?;
        let state = Arc::new(tokio::sync::RwLock::new(state));

        let total_size = state.read().await.total_size;
        let _ = event_tx
            .send(ProgressEvent::Initialized { total_size })
            .await;

        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent));
        let speed_limiter = Arc::new(TokioRwLock::new(SpeedLimiter::new(self.config.speed_limit)));
        let mut workers = Vec::new();

        let chunks_count = { state.read().await.chunks.len() };
        for i in 0..chunks_count {
            let permit = semaphore.clone().acquire_owned().await?;
            let state_c = Arc::clone(&state);
            let client_c = self.client.clone();
            let url_c = url.to_string();
            let dest_c = dest_path.clone();
            let state_file_c = state_path.clone();
            let tx_c = event_tx.clone();
            let speed_limiter_c = Arc::clone(&speed_limiter);
            let headers = self.config.headers.clone();
            let user_agent = self.config.user_agent.clone();

            workers.push(tokio::spawn(async move {
                let res = Self::download_chunk(
                    i,
                    client_c,
                    url_c,
                    dest_c,
                    state_file_c,
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
        event_tx.send(ProgressEvent::Finished).await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    /// 下载单个分块
    async fn download_chunk(
        index: usize,
        client: reqwest::Client,
        url: String,
        dest: std::path::PathBuf,
        state_file: std::path::PathBuf,
        state_lock: Arc<tokio::sync::RwLock<DownloadState>>,
        tx: mpsc::Sender<ProgressEvent>,
        speed_limiter: Arc<TokioRwLock<SpeedLimiter>>,
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
                .get(&url)
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
                        let chunk_data = item.map_err(|e| anyhow!("Stream error: {}", e))?;
                        file.write_all(&chunk_data).await?;

                        let len = chunk_data.len() as u64;
                        current_idx += len;

                        // 速度限制
                        speed_limiter.write().await.wait(len).await;

                        // 更新内存状态
                        {
                            let mut s = state_lock.write().await;
                            s.chunks[index].current = current_idx;
                        }

                        let _ = tx
                            .send(ProgressEvent::ChunkUpdated {
                                chunk_index: index,
                                delta: len,
                            })
                            .await;

                        // 保存状态
                        let state = state_lock.read().await;
                        state.save(&state_file).await?;
                    }

                    let mut s = state_lock.write().await;
                    s.chunks[index].is_finished = true;
                    return Ok(());
                }
                _ => {
                    retry_count += 1;
                    if retry_count > MAX_RETRIES {
                        return Err(anyhow!(
                            "Chunk {} failed after {} retries",
                            index,
                            MAX_RETRIES
                        ));
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

        // 创建新状态
        let res = self.client.head(url).send().await?;
        let total_size = res
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
            .ok_or_else(|| anyhow!("Server must support Content-Length"))?;

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
            total_size,
            chunks,
        };
        state.save(state_path).await?;
        Ok(state)
    }
}
