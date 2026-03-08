use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

pub const DEFAULT_APP_DIR: &str = ".aex";

#[derive(Debug, Clone)]
pub struct Storage {
    pub app_dir: PathBuf,
    pub files: HashMap<String, PathBuf>,
}

impl Storage {
    pub fn new(data_dir: Option<&str>) -> Self {
        // 调用提取出来的逻辑
        let app_dir = Self::resolve_app_dir(data_dir, dirs_next::data_dir());

        println!("Storage app dir: {:?}", app_dir);
        let _ = fs::create_dir_all(&app_dir);

        Storage {
            app_dir,
            files: HashMap::new(),
        }
    }

    // 将逻辑提取为纯函数，方便注入测试
    pub fn resolve_app_dir(custom_dir: Option<&str>, sys_data_dir: Option<PathBuf>) -> PathBuf {
        if let Some(dir) = custom_dir {
            PathBuf::from(dir)
        } else {
            sys_data_dir
                .map(|dir| dir.join(DEFAULT_APP_DIR))
                .unwrap_or_else(|| PathBuf::from(DEFAULT_APP_DIR)) // 这里就是你要测的备选逻辑
        }
    }

    pub fn save<T>(&self, k: &String, t: &T) -> anyhow::Result<()>
    where
        T: Serialize,
    {
        let json = serde_json::to_vec_pretty(t)?;
        let default_path = PathBuf::from(DEFAULT_APP_DIR);
        let file = self.files.get(k).unwrap_or_else(|| &default_path);
        fs::write(file, json)?;
        Ok(())
    }

    pub fn read<T>(&self, k: &String) -> anyhow::Result<Option<T>>
    where
        T: for<'a> Deserialize<'a>,
    {
        let default_path = PathBuf::from(DEFAULT_APP_DIR);
        let file = self.files.get(k).unwrap_or_else(|| &default_path);
        println!("Reading address from {:?}", &file);
        if !file.exists() {
            println!("Address file does not exist.");
            return Ok(None);
        }

        let bytes = fs::read(&file)?;
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    pub fn dir(&self) -> &str {
        let str = self.app_dir.as_os_str().to_str().unwrap();
        str
    }
}
