//! Helpers for integration tests.

use pushkind_common::db::{DbPool, establish_connection_pool};

/// Temporary database used in integration tests.
pub struct TestDb {
    filename: String,
    pool: Option<DbPool>,
}

impl TestDb {
    #[allow(dead_code)]
    pub fn new(filename: &str) -> Self {
        std::fs::remove_file(filename).ok(); // Clean up old DB

        let pool =
            establish_connection_pool(filename).expect("Failed to establish SQLite connection.");
        let _conn = pool
            .get()
            .expect("Failed to get SQLite connection from pool.");
        TestDb {
            filename: filename.to_string(),
            pool: Some(pool),
        }
    }
    #[allow(dead_code)]
    pub fn pool(&self) -> DbPool {
        self.pool
            .as_ref()
            .expect("TestDb pool should be available")
            .clone()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            drop(pool);
        }

        std::fs::remove_file(&self.filename).ok();
        std::fs::remove_file(format!("{}-shm", &self.filename)).ok();
        std::fs::remove_file(format!("{}-wal", &self.filename)).ok();
    }
}
