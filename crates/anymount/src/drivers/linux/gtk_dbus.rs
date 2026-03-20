//! org.gtk.Actions and org.gtk.Menus D-Bus interfaces for account context menu.
//!
//! Protocol: <https://wiki.gnome.org/Projects/GLib/GApplication/DBusAPI>.

use std::collections::HashMap;
use zbus::zvariant::{OwnedValue, Value};

pub const ACTION_OPEN_FOLDER: &str = "openfolder";
pub const ACTION_FREE_LOCAL_CACHE: &str = "freelocalcache";

const SUBSCRIPTION_GROUP: u32 = 0;
const MENU_ID: u32 = 0;

pub fn action_names() -> &'static [&'static str] {
    &[ACTION_OPEN_FOLDER, ACTION_FREE_LOCAL_CACHE]
}

pub fn menu_items() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Open in file manager", ACTION_OPEN_FOLDER),
        ("Free local cache", ACTION_FREE_LOCAL_CACHE),
    ]
}

pub fn describe_action(enabled: bool) -> (bool, String, Vec<OwnedValue>) {
    (enabled, String::new(), Vec::new())
}

pub fn build_start_reply() -> Vec<(u32, u32, Vec<HashMap<String, OwnedValue>>)> {
    let items: Vec<HashMap<String, OwnedValue>> = menu_items()
        .into_iter()
        .map(|(label, action)| {
            let mut attrs = HashMap::new();
            attrs.insert(
                "label".to_string(),
                OwnedValue::try_from(Value::from(label)).expect("label to OwnedValue"),
            );
            attrs.insert(
                "action".to_string(),
                OwnedValue::try_from(Value::from(action)).expect("action to OwnedValue"),
            );
            attrs
        })
        .collect();
    vec![(SUBSCRIPTION_GROUP, MENU_ID, items)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_names_include_open_and_free_cache() {
        let names = action_names();
        assert!(names.contains(&ACTION_OPEN_FOLDER));
        assert!(names.contains(&ACTION_FREE_LOCAL_CACHE));
    }

    #[test]
    fn menu_items_match_actions() {
        let items = menu_items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].1, ACTION_OPEN_FOLDER);
        assert_eq!(items[1].1, ACTION_FREE_LOCAL_CACHE);
    }

    #[test]
    fn build_start_reply_returns_one_menu() {
        let reply = build_start_reply();
        assert_eq!(reply.len(), 1);
        assert_eq!(reply[0].0, SUBSCRIPTION_GROUP);
        assert_eq!(reply[0].1, MENU_ID);
        assert_eq!(reply[0].2.len(), 2);
    }
}
