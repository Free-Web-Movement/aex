#[cfg(test)]
mod tests {
    use aex::storage::{DEFAULT_APP_DIR, Storage};
    use serde::{Deserialize, Serialize};

    use std::{fs, path::PathBuf};

    // 辅助函数：创建一个随机的临时测试目录名
    fn get_temp_test_dir() -> String {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("target/test_storage_{}", timestamp)
    }

    #[test]
    fn test_new_storage_with_custom_dir() {
        let test_dir = get_temp_test_dir();

        // 执行创建
        let storage = Storage::new(Some(&test_dir));

        // 1. 验证路径是否正确转换
        assert_eq!(storage.app_dir, PathBuf::from(&test_dir));

        // 2. 验证目录是否真的在文件系统中创建了
        assert!(storage.app_dir.exists(), "目录应当被创建");
        assert!(storage.app_dir.is_dir(), "应当是一个目录");

        // 清理测试数据
        let _ = fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_new_storage_default_dir() {
        // 执行默认创建
        let storage = Storage::new(None);

        // 验证路径末尾是否包含我们定义的 DEFAULT_APP_DIR
        let path_str = storage.app_dir.to_string_lossy();
        assert!(path_str.contains(DEFAULT_APP_DIR));

        // 验证目录创建逻辑
        assert!(storage.app_dir.exists());
    }

    #[test]
    fn test_files_map_initialization() {
        let storage = Storage::new(Some(&get_temp_test_dir()));

        // 验证 HashMap 是否初始化为空
        assert!(storage.files.is_empty());

        // 清理
        let _ = fs::remove_dir_all(&storage.app_dir);
    }

    #[test]
    fn test_file_path_joining() {
        let test_dir = get_temp_test_dir();
        let mut storage = Storage::new(Some(&test_dir));

        // 模拟添加一个 session 配置文件路径
        let file_name = "session_key.bin".to_string();
        let file_path = storage.app_dir.join(&file_name);
        storage.files.insert(file_name.clone(), file_path.clone());

        assert!(storage.files.contains_key(&file_name));
        assert_eq!(
            storage.files.get(&file_name).unwrap(),
            &storage.app_dir.join("session_key.bin")
        );

        let _ = fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_resolve_app_dir_fallback() {
        // 模拟场景：既没有自定义路径，系统 data_dir 也返回 None
        let result = Storage::resolve_app_dir(None, None);

        // 验证是否成功走到了备选逻辑：PathBuf::from(DEFAULT_APP_DIR)
        assert_eq!(result, PathBuf::from(DEFAULT_APP_DIR));
        assert_eq!(result.to_str().unwrap(), ".aex");
    }

    #[test]
    fn test_resolve_app_dir_sys_data() {
        // 模拟场景：系统返回了 /home/user/.local/share
        let mock_sys_dir = Some(PathBuf::from("/mock/data"));
        let result = Storage::resolve_app_dir(None, mock_sys_dir);

        // 验证是否正确拼接
        let expected = PathBuf::from("/mock/data").join(DEFAULT_APP_DIR);
        assert_eq!(result, expected);
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestData {
        id: u32,
        name: String,
    }

    fn setup_storage() -> (Storage, String) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let test_dir = format!("target/test_storage_{}", timestamp);
        (Storage::new(Some(&test_dir)), test_dir)
    }

    #[test]
    fn test_save_and_read_success() -> anyhow::Result<()> {
        let (mut storage, test_dir) = setup_storage();

        // 准备数据
        let key = "user_1".to_string();
        let data = TestData {
            id: 42,
            name: "Alice".to_string(),
        };

        // 手动插入路径到 files (模拟实际使用场景)
        let file_path = storage.app_dir.join("user_1.json");
        storage.files.insert(key.clone(), file_path.clone());

        // 测试保存
        storage.save(key.clone(), &data)?;

        // 测试读取
        let loaded: Option<TestData> = storage.read(key)?;

        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap(), data);

        // 清理
        let _ = fs::remove_dir_all(test_dir);
        Ok(())
    }

    #[test]
    fn test_read_non_existent_file() -> anyhow::Result<()> {
        let (storage, test_dir) = setup_storage();

        // 读取一个未定义的 key
        let result: Option<TestData> = storage.read("none".to_string())?;

        assert!(result.is_none());

        let _ = fs::remove_dir_all(test_dir);
        Ok(())
    }

    #[test]
    fn test_dir_string_output() {
        // 使用一个确定的路径名
        let custom_path = "test_data_folder";
        let storage = Storage::new(Some(custom_path));

        // 验证 dir() 返回的内容
        let dir_str = storage.dir();

        // 在 Unix 上应该是 "test_data_folder"
        // 在 Windows 上可能是 "test_data_folder" (PathBuf 会自动处理)
        assert!(dir_str.contains("test_data_folder"));

        // 清理创建的目录
        let _ = std::fs::remove_dir(custom_path);
    }

    #[test]
    fn test_dir_consistency() {
        // 验证 dir() 返回的值和内部 PathBuf 转换出来的字符串是一致 of
        let storage = Storage::new(None);
        let expected = storage
            .app_dir
            .to_str()
            .expect("Path should be valid UTF-8");

        assert_eq!(storage.dir(), expected);
    }
}
