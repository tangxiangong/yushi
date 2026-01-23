use super::app::{App, InputMode, SelectedPanel};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
};
use yushi_core::{TaskStatus, nbyte::Storage};

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 标题
            Constraint::Min(10),   // 主内容
            Constraint::Length(3), // 状态栏
            Constraint::Length(5), // 帮助
        ])
        .split(f.area());

    draw_header(f, chunks[0]);
    draw_main_content(f, app, chunks[1]);
    draw_status_bar(f, app, chunks[2]);
    draw_help(f, app, chunks[3]);
}

fn draw_header(f: &mut Frame, area: Rect) {
    let title = Paragraph::new("YuShi 下载管理器")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, area);
}

fn draw_main_content(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    draw_task_list(f, app, chunks[0]);
    draw_task_details(f, app, chunks[1]);
}

fn draw_task_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let status_icon = match task.status {
                TaskStatus::Pending => "⏸",
                TaskStatus::Downloading => "⬇",
                TaskStatus::Paused => "⏸",
                TaskStatus::Completed => "✓",
                TaskStatus::Failed => "✗",
                TaskStatus::Cancelled => "⊗",
            };

            let status_color = match task.status {
                TaskStatus::Pending => Color::Yellow,
                TaskStatus::Downloading => Color::Blue,
                TaskStatus::Paused => Color::Magenta,
                TaskStatus::Completed => Color::Green,
                TaskStatus::Failed => Color::Red,
                TaskStatus::Cancelled => Color::DarkGray,
            };

            let progress = if task.total_size > 0 {
                (task.downloaded as f64 / task.total_size as f64 * 100.0) as u16
            } else {
                0
            };

            let filename = task
                .dest
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            let size_str = if task.total_size > 0 {
                format!(
                    "{} / {}",
                    Storage::from_bytes(task.downloaded),
                    Storage::from_bytes(task.total_size)
                )
            } else {
                "未知大小".to_string()
            };

            let speed_str = if task.speed > 0 {
                format!(" @ {}/s", Storage::from_bytes(task.speed))
            } else {
                String::new()
            };

            let content = vec![
                Line::from(vec![
                    Span::styled(
                        format!("{} ", status_icon),
                        Style::default()
                            .fg(status_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(filename, Style::default().add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::raw(format!("  {}%  ", progress)),
                    Span::styled(size_str, Style::default().fg(Color::Gray)),
                    Span::styled(speed_str, Style::default().fg(Color::Cyan)),
                ]),
            ];

            let style = if i == app.selected_index {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(content).style(style)
        })
        .collect();

    let border_style = if app.selected_panel == SelectedPanel::TaskList {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("任务列表")
            .border_style(border_style),
    );

    f.render_widget(list, area);
}

fn draw_task_details(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.selected_panel == SelectedPanel::Details {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    if let Some(task) = app.get_selected_task() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(3)])
            .split(area);

        // 详细信息
        let mut lines = vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&task.id),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "URL: ",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(format!("  {}", task.url)),
            Line::from(""),
            Line::from(vec![Span::styled(
                "输出: ",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(format!("  {}", task.dest.display())),
            Line::from(""),
            Line::from(vec![
                Span::styled("状态: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{:?}", task.status),
                    Style::default().fg(match task.status {
                        TaskStatus::Completed => Color::Green,
                        TaskStatus::Failed => Color::Red,
                        TaskStatus::Downloading => Color::Blue,
                        _ => Color::Yellow,
                    }),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("优先级: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!("{:?}", task.priority)),
            ]),
        ];

        if let Some(error) = &task.error {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "错误: ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(format!("  {}", error)));
        }

        if let Some(eta) = task.eta {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("预计剩余: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!("{}s", eta)),
            ]));
        }

        let details = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("任务详情")
                    .border_style(border_style),
            )
            .wrap(Wrap { trim: true });

        f.render_widget(details, chunks[0]);

        // 进度条
        let progress = if task.total_size > 0 {
            (task.downloaded as f64 / task.total_size as f64 * 100.0) as u16
        } else {
            0
        };

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("进度"))
            .gauge_style(
                Style::default()
                    .fg(Color::Cyan)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .percent(progress)
            .label(format!("{}%", progress));

        f.render_widget(gauge, chunks[1]);
    } else {
        let empty = Paragraph::new("没有选中的任务")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("任务详情")
                    .border_style(border_style),
            )
            .alignment(Alignment::Center);
        f.render_widget(empty, area);
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status_text = if app.input_mode == InputMode::AddUrl {
        format!("输入: {}", app.input_buffer)
    } else {
        app.status_message.clone()
    };

    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL).title("状态"));

    f.render_widget(status, area);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let help_text = match app.input_mode {
        InputMode::Normal => {
            "q:退出 | ↑↓/jk:导航 | Tab:切换面板 | a:添加 | p:暂停/恢复 | c:取消 | d:删除 | C:清空 | r:刷新"
        }
        InputMode::AddUrl => "Enter:确认 | Esc:取消 | 格式: URL|输出路径|优先级(high/normal/low)",
    };

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title("帮助"));

    f.render_widget(help, area);
}
