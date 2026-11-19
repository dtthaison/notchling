use notchling_local_api::ops;
use serde_json::json;

use crate::acting::Request;

pub fn list() -> Request {
    Request {
        op: ops::ORGANIZATIONS_LIST,
        args: json!({}),
    }
}

pub fn create(name: &str, color: Option<&str>) -> Request {
    let mut args = json!({ "name": name });
    if let Some(color) = color {
        args["color"] = json!(color);
    }
    Request {
        op: ops::ORGANIZATIONS_CREATE,
        args,
    }
}

pub fn rename(id: &str, name: &str) -> Request {
    Request {
        op: ops::ORGANIZATIONS_RENAME,
        args: json!({ "id": id, "name": name }),
    }
}

pub fn set_color(id: &str, color: &str) -> Request {
    Request {
        op: ops::ORGANIZATIONS_COLOR_SET,
        args: json!({ "id": id, "color": color }),
    }
}

pub fn set_folders(id: &str, folders: &[String]) -> Request {
    Request {
        op: ops::ORGANIZATIONS_FOLDERS_SET,
        args: json!({ "id": id, "folders": folders }),
    }
}

pub fn preview_folders(id: &str, folders: &[String]) -> Request {
    Request {
        op: ops::ORGANIZATIONS_FOLDERS_PREVIEW,
        args: json!({ "id": id, "folders": folders }),
    }
}

pub fn delete(id: &str) -> Request {
    Request {
        op: ops::ORGANIZATIONS_DELETE,
        args: json!({ "id": id }),
    }
}
