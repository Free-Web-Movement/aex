use ahash::AHashMap;

const MAX_PARAMS: usize = 8;

#[derive(Debug, Clone)]
pub struct SmallParams {
    entries: [(String, String); MAX_PARAMS],
    len: usize,
}

impl Default for SmallParams {
    fn default() -> Self {
        const EMPTY: String = String::new();
        Self {
            entries: [
                (EMPTY, EMPTY),
                (EMPTY, EMPTY),
                (EMPTY, EMPTY),
                (EMPTY, EMPTY),
                (EMPTY, EMPTY),
                (EMPTY, EMPTY),
                (EMPTY, EMPTY),
                (EMPTY, EMPTY),
            ],
            len: 0,
        }
    }
}

impl SmallParams {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn with_capacity(_cap: usize) -> Self {
        Self::default()
    }

    #[inline]
    pub fn insert(&mut self, key: String, value: String) {
        if self.len < MAX_PARAMS {
            self.entries[self.len] = (key, value);
            self.len += 1;
        }
    }

    #[inline]
    pub fn get(&self, key: &str) -> Option<&str> {
        for i in 0..self.len {
            if self.entries[i].0 == key {
                return Some(&self.entries[i].1);
            }
        }
        None
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub unsafe fn into_map_unchecked(self) -> AHashMap<String, String> {
        let mut map = AHashMap::with_capacity(self.len);
        let mut i = 0usize;
        while i < self.len {
            unsafe {
                let k = std::ptr::read(&self.entries[i].0 as *const String);
                let v = std::ptr::read(&self.entries[i].1 as *const String);
                map.insert(k, v);
            }
            i += 1;
        }
        std::mem::forget(self);
        map
    }
}


impl From<SmallParams> for AHashMap<String, String> {
    fn from(small: SmallParams) -> Self {
        unsafe { small.into_map_unchecked() }
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
