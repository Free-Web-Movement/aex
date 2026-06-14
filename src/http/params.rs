use ahash::AHashMap;

#[derive(Debug, Clone, Default)]
pub struct SmallParams {
    entries: Vec<(String, String)>,
}

impl SmallParams {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
        }
    }

    #[inline]
    pub fn insert(&mut self, key: String, value: String) {
        self.entries.push((key, value));
    }

    #[inline]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl From<SmallParams> for AHashMap<String, String> {
    fn from(small: SmallParams) -> Self {
        small.entries.into_iter().collect()
    }
}

#[derive(Debug, Clone)]
pub struct Params {
    pub url: String,
    pub data: Option<AHashMap<String, String>>,
    pub query: AHashMap<String, Vec<String>>,
    pub form: Option<AHashMap<String, Vec<String>>>,
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

    pub fn parse_pairs(pairs: &str) -> AHashMap<String, Vec<String>> {
        let mut map: AHashMap<String, Vec<String>> = AHashMap::new();
        for (k, v) in form_urlencoded::parse(pairs.as_bytes()) {
            map.entry(k.into_owned()).or_default().push(v.into_owned());
        }
        map
    }

    pub fn set_form(&mut self, form: &str) {
        self.form = Some(Self::parse_pairs(form));
    }
}
