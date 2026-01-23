use crate::{
    cli::{ConfigArgs, ConfigCommands},
    config::Config,
    ui::{print_error, print_info, print_success},
};
use anyhow::Result;
use console::style;

pub async fn execute(args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigCommands::Show => show_config().await,
        ConfigCommands::Set { key, value } => set_config(key, value).await,
        ConfigCommands::Reset => reset_config().await,
    }
}

async fn show_config() -> Result<()> {
    let config = Config::load()?;

    println!("\n{}", style("当前配置").bold().underlined());
    println!();
    println!("  默认并发连接数: {}", config.default_connections);
    println!("  默认最大任务数: {}", config.default_max_tasks);
    println!("  默认输出目录: {}", config.default_output_dir.display());

    if let Some(ua) = &config.user_agent {
        println!("  User-Agent: {}", ua);
    }

    if let Some(proxy) = &config.proxy {
        println!("  代理: {}", proxy);
    }

    if let Some(limit) = &config.speed_limit {
        println!("  速度限制: {}", limit);
    }

    println!();
    println!("配置文件: {}", Config::config_path()?.display());
    println!("队列文件: {}", Config::queue_state_path()?.display());

    Ok(())
}

async fn set_config(key: String, value: String) -> Result<()> {
    let mut config = Config::load()?;

    match key.as_str() {
        "connections" => {
            config.default_connections = value.parse()?;
            print_success(&format!("默认并发连接数已设置为: {}", value));
        }
        "max_tasks" => {
            config.default_max_tasks = value.parse()?;
            print_success(&format!("默认最大任务数已设置为: {}", value));
        }
        "output_dir" => {
            config.default_output_dir = value.into();
            print_success(&format!(
                "默认输出目录已设置为: {}",
                config.default_output_dir.display()
            ));
        }
        "user_agent" => {
            config.user_agent = Some(value.clone());
            print_success(&format!("User-Agent 已设置为: {}", value));
        }
        "proxy" => {
            config.proxy = Some(value.clone());
            print_success(&format!("代理已设置为: {}", value));
        }
        "speed_limit" => {
            config.speed_limit = Some(value.clone());
            print_success(&format!("速度限制已设置为: {}", value));
        }
        _ => {
            print_error(&format!("未知的配置项: {}", key));
            print_info(
                "可用的配置项: connections, max_tasks, output_dir, user_agent, proxy, speed_limit",
            );
            return Ok(());
        }
    }

    config.save()?;
    Ok(())
}

async fn reset_config() -> Result<()> {
    let config = Config::default();
    config.save()?;
    print_success("配置已重置为默认值");
    Ok(())
}
