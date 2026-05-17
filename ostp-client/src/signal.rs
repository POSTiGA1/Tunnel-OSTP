use anyhow::Result;

#[cfg(unix)]
pub async fn wait_for_shutdown_signal() -> Result<()> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => {
            tracing::info!("Received SIGTERM, shutting down");
        }
        _ = sigint.recv() => {
            tracing::info!("Received SIGINT, shutting down");
        }
    }

    Ok(())
}

#[cfg(not(unix))]
pub async fn wait_for_shutdown_signal() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
        let mut c_c = ctrl_c()?;
        let mut c_close = ctrl_close()?;
        let mut c_break = ctrl_break()?;

        tokio::select! {
            res = c_c.recv() => {
                tracing::info!("Received Ctrl+C, shutting down");
                if res.is_none() {
                    std::future::pending::<()>().await;
                }
            }
            res = c_close.recv() => {
                tracing::info!("Received console close event, shutting down");
                if res.is_none() {
                    std::future::pending::<()>().await;
                }
            }
            res = c_break.recv() => {
                tracing::info!("Received Ctrl+Break, shutting down");
                if res.is_none() {
                    std::future::pending::<()>().await;
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        tokio::signal::ctrl_c().await?;
    }
    Ok(())
}
