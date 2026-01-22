use crate::types::ChecksumType;
use anyhow::Result;
use fs_err::tokio as fs;
use md5::{Digest, Md5};
use sha2::Sha256;
use std::path::Path;
use tokio::io::AsyncReadExt;

/// 速度限制器
pub struct SpeedLimiter {
    limit: Option<u64>,
    last_check: std::time::Instant,
    bytes_in_period: u64,
}

impl SpeedLimiter {
    /// 创建新的速度限制器
    pub fn new(limit: Option<u64>) -> Self {
        Self {
            limit,
            last_check: std::time::Instant::now(),
            bytes_in_period: 0,
        }
    }

    /// 等待以满足速度限制
    pub async fn wait(&mut self, bytes: u64) {
        if let Some(limit) = self.limit {
            self.bytes_in_period += bytes;
            let elapsed = self.last_check.elapsed();

            if elapsed.as_secs() >= 1 {
                // 重置计数器
                self.last_check = std::time::Instant::now();
                self.bytes_in_period = 0;
            } else if self.bytes_in_period > limit {
                // 需要等待
                let wait_time = std::time::Duration::from_secs(1) - elapsed;
                tokio::time::sleep(wait_time).await;
                self.last_check = std::time::Instant::now();
                self.bytes_in_period = 0;
            }
        }
    }
}

/// 速度计算器
pub struct SpeedCalculator {
    start_time: std::time::Instant,
    last_update: std::time::Instant,
    last_bytes: u64,
    current_speed: u64,
}

impl SpeedCalculator {
    /// 创建新的速度计算器
    pub fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            start_time: now,
            last_update: now,
            last_bytes: 0,
            current_speed: 0,
        }
    }

    /// 更新速度统计
    pub fn update(&mut self, total_downloaded: u64) -> u64 {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();

        if elapsed >= 1.0 {
            let bytes_diff = total_downloaded.saturating_sub(self.last_bytes);
            self.current_speed = (bytes_diff as f64 / elapsed) as u64;
            self.last_update = now;
            self.last_bytes = total_downloaded;
        }

        self.current_speed
    }

    /// 计算 ETA（预计剩余时间，秒）
    pub fn calculate_eta(&self, downloaded: u64, total: u64) -> Option<u64> {
        if self.current_speed == 0 || downloaded >= total {
            return None;
        }

        let remaining = total - downloaded;
        Some(remaining / self.current_speed)
    }

    /// 获取平均速度
    pub fn average_speed(&self, total_downloaded: u64) -> u64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            (total_downloaded as f64 / elapsed) as u64
        } else {
            0
        }
    }
}

impl Default for SpeedCalculator {
    fn default() -> Self {
        Self::new()
    }
}

/// 文件校验
pub async fn verify_file(path: &Path, checksum: &ChecksumType) -> Result<bool> {
    let mut file = fs::File::open(path).await?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;

    let result = match checksum {
        ChecksumType::Md5(expected) => {
            let mut hasher = Md5::new();
            hasher.update(&buffer);
            let hash = hex::encode(hasher.finalize());
            hash.eq_ignore_ascii_case(expected)
        }
        ChecksumType::Sha256(expected) => {
            let mut hasher = Sha256::new();
            hasher.update(&buffer);
            let hash = hex::encode(hasher.finalize());
            hash.eq_ignore_ascii_case(expected)
        }
    };

    Ok(result)
}

/// 自动重命名文件以避免冲突
pub fn auto_rename(path: &Path) -> std::path::PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));

    let mut counter = 1;
    loop {
        let new_name = if ext.is_empty() {
            format!("{} ({})", stem, counter)
        } else {
            format!("{} ({}).{}", stem, counter, ext)
        };

        let new_path = parent.join(new_name);
        if !new_path.exists() {
            return new_path;
        }
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_rename() {
        let path = Path::new("/tmp/test.txt");
        let renamed = auto_rename(path);
        // 如果文件不存在，应该返回原路径
        assert_eq!(renamed, path);
    }

    #[tokio::test]
    async fn test_speed_calculator() {
        let mut calc = SpeedCalculator::new();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 模拟下载了 1MB
        let speed = calc.update(1024 * 1024);
        // 速度应该大于 0
        assert!(speed > 0);
    }
}
