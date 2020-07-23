pub mod idbstore;
pub mod memstore;

use async_trait::async_trait;
use std::fmt;

#[derive(Debug)]
pub enum StoreError {
    Str(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Str(s) => write!(f, "{}", s),
        }
    }
}

type Result<T> = std::result::Result<T, StoreError>;

#[async_trait(?Send)]
pub trait Store {
    async fn read<'a>(&'a self) -> Result<Box<dyn Read + 'a>>;
    async fn write<'a>(&'a self) -> Result<Box<dyn Write + 'a>>;

    async fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        let wt = self.write().await?;
        wt.put(key, value).await?;
        Ok(wt.commit().await?)
    }

    async fn has(&self, key: &str) -> Result<bool> {
        Ok(self.read().await?.has(key).await?)
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.read().await?.get(key).await?)
    }
}

#[async_trait(?Send)]
pub trait Read {
    async fn has(&self, key: &str) -> Result<bool>;
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
}

#[async_trait(?Send)]
pub trait Write: Read {
    fn as_read(&self) -> &dyn Read;

    async fn put(&self, key: &str, value: &[u8]) -> Result<()>;
    async fn del(&self, key: &str) -> Result<()>;

    async fn commit(self: Box<Self>) -> Result<()>;
    async fn rollback(self: Box<Self>) -> Result<()>;
}

pub mod trait_tests {
    use super::{Store, StoreError};

    pub async fn store(store: &mut dyn Store) -> std::result::Result<(), StoreError> {
        // Test put/has/get, which use read() and write() for one-shot txs.
        assert!(!store.has("foo").await?);
        assert_eq!(None, store.get("foo").await?);

        store.put("foo", b"bar").await?;
        assert!(store.has("foo").await?);
        assert_eq!(Some(b"bar".to_vec()), store.get("foo").await?);

        store.put("foo", b"baz").await?;
        assert!(store.has("foo").await?);
        assert_eq!(Some(b"baz".to_vec()), store.get("foo").await?);

        assert!(!store.has("baz").await?);
        assert_eq!(None, store.get("baz").await?);
        store.put("baz", b"bat").await?;
        assert!(store.has("baz").await?);
        assert_eq!(Some(b"bat".to_vec()), store.get("baz").await?);

        Ok(())
    }

    pub async fn read_transaction(store: &mut dyn Store) -> std::result::Result<(), StoreError> {
        store.put("k1", b"v1").await?;

        let rt = store.read().await?;
        assert!(rt.has("k1").await?);
        assert_eq!(Some(b"v1".to_vec()), rt.get("k1").await?);

        Ok(())
    }

    pub async fn write_transaction(store: &mut dyn Store) -> std::result::Result<(), StoreError> {
        store.put("k1", b"v1").await?;
        store.put("k2", b"v2").await?;

        // Test put then commit.
        let wt = store.write().await?;
        assert!(wt.has("k1").await?);
        assert!(wt.has("k2").await?);
        wt.put("k1", b"overwrite").await?;
        wt.commit().await?;
        assert_eq!(Some(b"overwrite".to_vec()), store.get("k1").await?);
        assert_eq!(Some(b"v2".to_vec()), store.get("k2").await?);

        // Test put then rollback.
        let wt = store.write().await?;
        wt.put("k1", b"should be rolled back").await?;
        wt.rollback().await?;
        assert_eq!(Some(b"overwrite".to_vec()), store.get("k1").await?);

        // Test del then commit.
        let wt = store.write().await?;
        wt.del("k1").await?;
        assert!(!wt.has("k1").await?);
        wt.commit().await?;
        assert!(!store.has("k1").await?);

        // Test del then rollback.
        assert_eq!(true, store.has("k2").await?);
        let wt = store.write().await?;
        wt.del("k2").await?;
        assert!(!wt.has("k2").await?);
        wt.rollback().await?;
        assert!(store.has("k2").await?);

        // Test overwrite multiple times then commit.
        let wt = store.write().await?;
        wt.put("k2", b"overwrite").await?;
        wt.del("k2").await?;
        wt.put("k2", b"final").await?;
        wt.commit().await?;
        assert_eq!(Some(b"final".to_vec()), store.get("k2").await?);

        // Test as_read.
        let wt = store.write().await?;
        wt.put("k2", b"new value").await?;
        let rt = wt.as_read();
        assert!(rt.has("k2").await?);
        assert_eq!(Some(b"new value".to_vec()), rt.get("k2").await?);

        Ok(())
    }

    pub async fn isolation(store: &mut dyn Store) {
        use async_std::future::timeout;
        use log::error;
        use std::time::Duration;

        // We don't get line numbers in stack traces in wasm so we use an error message
        // which is logged to console to identify the issue. AFAICT this does nothing
        // when running regular tests, but that's ok because we get useful stack traces
        // in that case.
        fn spew(msg: &str) {
            error!("{}", msg);
        }

        // Assert there can be multiple concurrent read txs...
        let r1 = store.read().await.unwrap();
        let r2 = store
            .read()
            .await
            .expect("should be able to open second read");
        // and that while outstanding they prevent write txs...
        let dur = Duration::from_millis(200);
        let w = store.write();
        if timeout(dur, w).await.is_ok() {
            spew("2 open read tx should have prevented new write");
            panic!();
        }
        // until both the reads are done...
        drop(r1);
        let w = store.write();
        if timeout(dur, w).await.is_ok() {
            spew("1 open read tx should have prevented new write");
            panic!();
        }
        drop(r2);
        let w = store.write().await.unwrap();

        // At this point we have a write tx outstanding. Assert that
        // we cannot open another write transaction.
        let w2 = store.write();
        if timeout(dur, w2).await.is_ok() {
            spew("1 open write tx should have prevented new write");
            panic!();
        }

        // The write tx is still outstanding, ensure we cannot open
        // a read tx until it is finished.
        let r = store.read();
        if timeout(dur, r).await.is_ok() {
            spew("1 open write tx should have prevented new read");
            panic!();
        }
        w.rollback().await.unwrap();
        let r = store.read().await.unwrap();
        assert!(!r.has("foo").await.unwrap());
    }
}
