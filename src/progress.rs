use tokio::sync::mpsc::UnboundedSender;

/// Print a message to stdout and optionally send it through a channel (for SSE streaming).
///
/// In CLI mode, `tx` is `None` and this just prints.
/// In web/serve mode, `tx` is `Some(sender)` so the message also reaches the SSE endpoint.
pub fn emit(tx: &Option<UnboundedSender<String>>, msg: &str) {
    println!("{msg}");
    if let Some(tx) = tx {
        let _ = tx.send(msg.to_string());
    }
}
