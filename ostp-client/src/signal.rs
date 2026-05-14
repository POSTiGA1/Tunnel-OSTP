use anyhow::Result;

#[cfg(unix)]
pub async fn wait_for_shutdown_signal() -> Result<()> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => {}
        _ = sigint.recv() => {}
    }

    Ok(())
}

#[cfg(not(unix))]
pub async fn wait_for_shutdown_signal() -> Result<()> {
    tokio::signal::ctrl_c().await?;
    Ok(())
}
