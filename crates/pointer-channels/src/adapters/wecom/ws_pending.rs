use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::oneshot;

static PENDING: OnceLock<Mutex<HashMap<String, oneshot::Sender<Value>>>> = OnceLock::new();

fn pending() -> &'static Mutex<HashMap<String, oneshot::Sender<Value>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register(req_id: String) -> oneshot::Receiver<Value> {
    let (tx, rx) = oneshot::channel();
    pending().lock().insert(req_id, tx);
    rx
}

pub fn dispatch_response(frame: &Value) -> bool {
    let req_id = frame
        .get("headers")
        .and_then(|h| h.get("req_id"))
        .and_then(|v| v.as_str());
    let Some(req_id) = req_id else {
        return false;
    };
    let tx = pending().lock().remove(req_id);
    if let Some(tx) = tx {
        let _ = tx.send(frame.clone());
        true
    } else {
        false
    }
}
