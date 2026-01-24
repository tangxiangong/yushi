//! YuShi - 高性能异步下载库
//!
//! 提供统一的下载和队列管理功能，支持断点续传、并发下载等特性。

pub mod downloader;
pub mod state;
pub mod types;
pub mod utils;

// 重新导出公共 API
pub use downloader::YuShi;
pub use types::{
    ChecksumType,
    // 回调类型
    CompletionCallback,

    Config,
    DownloadCallback,
    DownloadConfig,
    // 向后兼容别名
    DownloadTask,
    // 事件类型
    DownloaderEvent,
    Priority,
    ProgressEvent,
    QueueEvent,
    // 主要类型
    Task,
    TaskEvent,
    TaskPriority,
    // 枚举类型
    TaskStatus,
    VerificationEvent,
};
pub use utils::{SpeedCalculator, auto_rename, verify_file};
