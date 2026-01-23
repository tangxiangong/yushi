# YuShi Frontend Architecture

## 项目结构

```
src/
├── commands.ts          # Tauri 命令封装（类型安全）
├── types.ts            # TypeScript 类型定义
├── App.tsx             # 主应用组件
├── main.tsx            # 应用入口
├── index.css           # 全局样式和 DaisyUI 配置
├── components/         # React 组件
│   ├── Layout.tsx      # 主布局（侧边栏 + 内容区）
│   ├── Sidebar.tsx     # 侧边栏导航
│   ├── TaskItem.tsx    # 下载任务卡片
│   ├── AddTaskModal.tsx    # 添加任务模态框
│   └── SettingsModal.tsx   # 设置模态框
├── lib/
│   └── utils.ts        # 工具函数（cn 等）
└── utils/
    └── format.ts       # 格式化函数（字节、时间等）
```

## 核心模块说明

### 1. commands.ts - Tauri 命令封装

提供类型安全的 Tauri 后端命令调用接口。

**为什么需要这个文件？**

- ✅ **类型安全**：TypeScript 类型检查，避免运行时错误
- ✅ **代码提示**：IDE 自动补全和参数提示
- ✅ **集中管理**：所有后端调用在一个地方，易于维护
- ✅ **文档化**：JSDoc 注释提供清晰的 API 说明
- ✅ **重构友好**：修改命令名称只需要改一个地方

**可用命令：**

```typescript
// 任务管理
addTask(url: string, dest: string): Promise<string>
getTasks(): Promise<DownloadTask[]>
pauseTask(id: string): Promise<void>
resumeTask(id: string): Promise<void>
cancelTask(id: string): Promise<void>
removeTask(id: string): Promise<void>

// 配置管理
getConfig(): Promise<AppConfig>
updateConfig(config: AppConfig): Promise<void>
```

**使用示例：**

```typescript
import { addTask, getTasks } from "./commands";

// 添加任务
const taskId = await addTask("https://example.com/file.zip", "/downloads");

// 获取所有任务
const tasks = await getTasks();
```

### 2. types.ts - 类型定义

定义前后端共享的数据结构。

```typescript
export type TaskStatus =
  | "Pending"
  | "Downloading"
  | "Paused"
  | "Completed"
  | "Failed"
  | "Cancelled";

export interface DownloadTask {
  id: string;
  url: string;
  dest: string;
  status: TaskStatus;
  total_size: number;
  downloaded: number;
  speed: number;
  eta?: number;
  // ...
}
```

### 3. 组件架构

#### Layout 组件

- 响应式布局（抽屉式侧边栏）
- 顶部导航栏
- 主内容区域

#### Sidebar 组件

- 导航菜单（全部/下载中/已完成）
- 设置按钮
- 品牌标识

#### TaskItem 组件

- 下载任务卡片
- 进度条显示
- 操作按钮（暂停/恢复/取消/删除）
- 状态徽章

#### Modal 组件

- AddTaskModal：添加新任务
- SettingsModal：应用设置（主题、路径等）

## 样式系统

### Tailwind CSS v4 + DaisyUI

**配置方式：**

```css
/* src/index.css */
@import "tailwindcss";
@import "daisyui";

@layer base {
  /* DaisyUI 主题配置 */
  [data-theme="cupcake"] { ... }
  [data-theme="dim"] { ... }
}
```

**主题：**

- `cupcake` - 浅色主题
- `dim` - 深色主题
- 支持系统主题自动切换

**设计原则：**

- 使用 DaisyUI 语义化组件类
- 渐变和动画增强视觉效果
- 响应式设计，适配各种屏幕
- 一致的间距和圆角

## 事件系统

### Tauri 事件监听

```typescript
import { listen } from "@tauri-apps/api/event";

// 监听下载事件
listen<QueueEvent>("download-event", (event) => {
  const data = event.payload;
  // 处理事件：TaskProgress, TaskCompleted, TaskFailed 等
});
```

**事件类型：**

- `TaskAdded` - 任务添加
- `TaskStarted` - 任务开始
- `TaskProgress` - 下载进度更新
- `TaskCompleted` - 任务完成
- `TaskFailed` - 任务失败
- `TaskPaused` - 任务暂停
- `TaskResumed` - 任务恢复
- `TaskCancelled` - 任务取消

## 开发指南

## 配置系统

### AppConfig 结构

```typescript
interface AppConfig {
  default_download_path: string; // 默认下载路径
  max_concurrent_downloads: number; // 每个任务的最大连接数
  max_concurrent_tasks: number; // 同时运行的最大任务数
  chunk_size: number; // 分块大小（字节）
  timeout: number; // 连接超时（秒）
  user_agent: string; // 用户代理
  theme: string; // 主题 (light/dark/system)
}
```

### 配置文件位置

- **macOS**: `~/Library/Application Support/com.yushi.app/config.json`
- **Linux**: `~/.config/yushi/config.json`
- **Windows**: `%APPDATA%\yushi\config.json`

### 使用配置

```typescript
import { getConfig, updateConfig } from "./commands";

// 获取配置
const config = await getConfig();

// 更新配置
await updateConfig({
  ...config,
  max_concurrent_tasks: 5,
  default_download_path: "/new/path",
});
```

### 添加新命令

1. 在 Rust 侧添加命令（`src-tauri/src/lib.rs`）：

```rust
#[tauri::command]
async fn new_command(state: State<'_, AppState>) -> Result<(), String> {
    // 实现逻辑
    Ok(())
}
```

2. 注册命令：

```rust
.invoke_handler(tauri::generate_handler![
    // ... 其他命令
    new_command
])
```

3. 在 `src/commands.ts` 添加封装：

```typescript
export async function newCommand(): Promise<void> {
  return invoke<void>("new_command");
}
```

4. 在组件中使用：

```typescript
import { newCommand } from "../commands";

await newCommand();
```

### 最佳实践

1. **始终使用 commands.ts**
   - ❌ 不要直接调用 `invoke()`
   - ✅ 使用封装的命令函数

2. **类型安全**
   - 确保 TypeScript 类型与 Rust 类型匹配
   - 使用 `DownloadTask` 等共享类型

3. **错误处理**
   - 使用 try-catch 捕获命令错误
   - 提供用户友好的错误提示

4. **状态管理**
   - 使用 React hooks（useState, useEffect）
   - 通过事件监听实时更新状态

## 技术栈

- **框架**: React 19 + TypeScript
- **构建**: Vite 7
- **样式**: Tailwind CSS v4 + DaisyUI 5
- **图标**: Lucide React
- **桌面**: Tauri 2
- **后端**: Rust (yushi-core)
