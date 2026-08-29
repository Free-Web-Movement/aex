use ahash::AHashMap;
use aex::http::params::{Params, SmallParams};

#[test]
fn test_small_params_insert_get_len_clear() {
    let mut p = SmallParams::new();
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);

    p.insert("a".to_string(), "1".to_string());
    p.insert("b".to_string(), "2".to_string());
    assert!(!p.is_empty());
    assert_eq!(p.len(), 2);
    assert_eq!(p.get("a"), Some("1"));
    assert_eq!(p.get("b"), Some("2"));
    assert_eq!(p.get("c"), None);

    p.clear();
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    assert_eq!(p.get("a"), None);
}

#[test]
fn test_small_params_get_returns_first_duplicate() {
    let mut p = SmallParams::new();
    p.insert("k".to_string(), "first".to_string());
    p.insert("k".to_string(), "second".to_string());
    assert_eq!(p.get("k"), Some("first"));
    assert_eq!(p.len(), 2);
}

#[test]
fn test_small_params_with_capacity() {
    let p = SmallParams::with_capacity(16);
    assert!(p.is_empty());

    let mut p = SmallParams::with_capacity(0);
    p.insert("x".to_string(), "y".to_string());
    assert_eq!(p.get("x"), Some("y"));
}

#[test]
fn test_small_params_into_hashmap_last_wins() {
    let mut p = SmallParams::new();
    p.insert("dup".to_string(), "v1".to_string());
    p.insert("dup".to_string(), "v2".to_string());
    let map: AHashMap<String, String> = p.into();
    assert_eq!(map.get("dup").map(|s| s.as_str()), Some("v2"));
}

#[test]
fn test_params_new_edge_cases() {
    let p = Params::new("http://x/".to_string());
    assert!(p.query.is_empty());
    assert!(p.form.is_none());
    assert!(p.data.is_none());

    let p = Params::new("http://x/?".to_string());
    assert!(p.query.is_empty());
    assert_eq!(p.url, "http://x/?");
}

#[test]
fn test_params_parse_utf8_and_plus() {
    let parsed = Params::parse_pairs("q=%E4%B8%AD%E6%96%87&n=a+b&x=%26");
    assert_eq!(parsed.get("q").unwrap()[0], "中文");
    assert_eq!(parsed.get("n").unwrap()[0], "a b");
    assert_eq!(parsed.get("x").unwrap()[0], "&");
}

#[test]
fn test_params_parse_empty_string() {
    let parsed = Params::parse_pairs("");
    assert!(parsed.is_empty());
}

#[test]
fn test_params_set_form_overwrites() {
    let mut params = Params::new("http://api".to_string());
    params.set_form("user=alice");
    params.set_form("user=bob&role=admin");
    let form = params.form.as_ref().unwrap();
    assert_eq!(form.get("user").unwrap()[0], "bob");
    assert_eq!(form.get("role").unwrap()[0], "admin");
}
