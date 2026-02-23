use std::collections::HashMap;

/// URL 参数结构
#[derive(Debug, Clone)]
pub struct Params {
    /// 原始请求 URL，包括 query
    pub url: String,
    /// Path 参数，例如 /user/:id -> {"id": "123"}
    pub data: Option<HashMap<String, String>>,
    /// Query 参数，例如 ?active=true -> {"active": "true"}
    pub query: HashMap<String, Vec<String>>,
    pub form: Option<HashMap<String, Vec<String>>>,
}

impl Params {
    pub fn new(url: String) -> Self {
        let query = url
            .split_once('?')
            .map(|(_, qs)| Self::parse_pairs(qs))
            .unwrap_or_default();

        Self {
            url,
            data: None,
            query,
            form: None,
        }
    }

    pub fn parse_pairs(pairs: &str) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (k, v) in form_urlencoded::parse(pairs.as_bytes()) {
            map.entry(k.into_owned()).or_default().push(v.into_owned());
        }
        map
    }

    pub fn set_form(&mut self, form: &str) {
        self.form = Some(Self::parse_pairs(form));
    }

    /// 解析 form body，支持数组参数
    fn parse_form(form: &str) -> HashMap<String, Vec<String>> {
        Self::parse_pairs(form)
    }
}
