mod common;

#[test]
fn test_creates_and_removes_db_files() {
    let base = "test_in_memory_connection.db";

    {
        let test_db = common::TestDb::new(base);
        let conn = test_db.pool().get();
        assert!(conn.is_ok());
    }

    let db_path = std::path::Path::new(base);
    assert!(!db_path.exists());
    assert!(!std::path::Path::new(&format!("{base}-shm")).exists());
    assert!(!std::path::Path::new(&format!("{base}-wal")).exists());
}
