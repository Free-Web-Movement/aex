use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use crate::constants::server::DEFAULT_APP_DIR;

#[derive(Debug, Clone)]
pub struct Storage {
    pub app_dir: PathBuf,
}

impl Storage {
    pub fn new(data_dir: Option<&str>) -> Self {
        // 调用提取出来的逻辑
        let app_dir = Self::resolve_app_dir(data_dir, dirs_next::data_dir());
        let _ = fs::create_dir_all(&app_dir);
        Storage { app_dir }
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

    pub fn real_path(&self, k: &String) -> PathBuf {
        let mut path = PathBuf::from(&self.app_dir);
        path.push(k);
        path
    }

    pub fn save<T>(&self, k: &String, t: &T) -> anyhow::Result<()>
    where
        T: Serialize,
    {
        let json = serde_json::to_vec_pretty(t)?;
        let file = self.real_path(k);
        fs::write(file, json)?;
        Ok(())
    }

    pub fn read<T>(&self, k: &String) -> anyhow::Result<Option<T>>
    where
        T: for<'a> Deserialize<'a>,
    {
        let file = self.real_path(k);
        if !file.exists() {
            tracing::debug!("Address file does not exist.");
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
