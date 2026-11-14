//! When a served process is asked to stop: Ctrl-C, or the program that
//! started it going away.

/// Set to `1` by a program that starts `moss build --serve` and keeps its
/// standard input open: the server then stops when that input closes, which
/// is when the program is gone, however it went. Opt-in, because a shell job
/// or a script starts moss with its input already closed.
pub const STOP_ON_STDIN_CLOSE: &str = "MOSS_STOP_ON_STDIN_CLOSE";

/// Ctrl-C, or the starting program gone. Waited on once the first build is
/// done; a host that leaves during it is seen then.
pub async fn requested() {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = host_gone() => {}
    }
}

/// Never resolves unless the starting program asked to be followed.
async fn host_gone() {
    if std::env::var_os(STOP_ON_STDIN_CLOSE).is_none_or(|v| v != "1") {
        return std::future::pending().await;
    }
    static CLOSED: std::sync::OnceLock<tokio_util::sync::CancellationToken> = std::sync::OnceLock::new();
    CLOSED
        .get_or_init(|| {
            let closed = tokio_util::sync::CancellationToken::new();
            let signal = closed.clone();
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
                signal.cancel();
            });
            closed
        })
        .cancelled()
        .await
}
