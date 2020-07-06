use crate::kv::{Read, Result, Store, StoreError, Write};
use async_std::sync::{Arc, Condvar, Mutex};
use async_trait::async_trait;
use futures::channel::oneshot;
use futures::future::join_all;
use log::warn;
use std::collections::HashMap;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;
use web_sys::{IdbDatabase, IdbObjectStore, IdbTransaction};

impl From<String> for StoreError {
    fn from(err: String) -> StoreError {
        StoreError::Str(err)
    }
}

impl From<JsValue> for StoreError {
    fn from(err: JsValue) -> StoreError {
        // TODO(nate): Pick out a useful subset of this value.
        StoreError::Str(format!("{:?}", err))
    }
}

impl From<futures::channel::oneshot::Canceled> for StoreError {
    fn from(_e: futures::channel::oneshot::Canceled) -> StoreError {
        StoreError::Str("oneshot cancelled".to_string())
    }
}

pub struct IdbStore {
    idb: IdbDatabase,
}

const OBJECT_STORE: &str = "chunks";

impl IdbStore {
    pub async fn new(name: &str) -> Result<Option<IdbStore>> {
        let window = match web_sys::window() {
            Some(w) => w,
            None => return Ok(None),
        };
        let factory = match window.indexed_db()? {
            Some(f) => f,
            None => return Ok(None),
        };
        let request = factory.open(name)?;
        let (sender, receiver) = oneshot::channel::<()>();
        let callback = Closure::once(move || {
            if let Err(_) = sender.send(()) {
                warn!("oneshot send failed");
            }
        });
        let request_copy = request.clone();
        let onupgradeneeded = Closure::once(move |_event: web_sys::IdbVersionChangeEvent| {
            let result = match request_copy.result() {
                Ok(r) => r,
                Err(e) => {
                    warn!("Error before ugradeneeded: {:?}", e);
                    return;
                }
            };
            let db = web_sys::IdbDatabase::unchecked_from_js(result);

            if let Err(e) = db.create_object_store(OBJECT_STORE) {
                warn!("Create object store failed: {:?}", e);
            }
        });
        request.set_onsuccess(Some(callback.as_ref().unchecked_ref()));
        request.set_onerror(Some(callback.as_ref().unchecked_ref()));
        request.set_onupgradeneeded(Some(onupgradeneeded.as_ref().unchecked_ref()));
        receiver.await?;
        Ok(Some(IdbStore {
            idb: request.result()?.into(),
        }))
    }
}

#[async_trait(?Send)]
impl Store for IdbStore {
    async fn read<'a>(&'a self) -> Result<Box<dyn Read + 'a>> {
        Ok(Box::new(ReadTransaction::new(self)?))
    }

    async fn write<'a>(&'a self) -> Result<Box<dyn Write + 'a>> {
        Ok(Box::new(WriteTransaction::new(self)?))
    }

    async fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
        let tx = self
            .idb
            .transaction_with_str_and_mode(OBJECT_STORE, web_sys::IdbTransactionMode::Readwrite)?;

        let (sender, txdonereceiver) = oneshot::channel::<()>();
        let callback = Closure::once(move || {
            if let Err(_) = sender.send(()) {
                warn!("oneshot send failed");
            }
        });
        tx.set_oncomplete(Some(callback.as_ref().unchecked_ref()));

        let store = tx.object_store(OBJECT_STORE)?;
        let request = store.put_with_key(&js_sys::Uint8Array::from(value), &key.into())?;
        let (sender, receiver) = oneshot::channel::<()>();
        let putcallback = Closure::once(move || {
            if let Err(_) = sender.send(()) {
                warn!("oneshot send failed");
            }
        });
        request.set_onsuccess(Some(putcallback.as_ref().unchecked_ref()));
        request.set_onerror(Some(putcallback.as_ref().unchecked_ref()));
        receiver.await?;

        // TODO(nate): Move into WriteTransaction.commit().
        txdonereceiver.await?;
        Ok(())
    }

    async fn has(&self, key: &str) -> Result<bool> {
        let tx = self.idb.transaction_with_str(OBJECT_STORE)?;
        let store = tx.object_store(OBJECT_STORE)?;
        let request = store.count_with_key(&key.into())?;
        let (sender, receiver) = oneshot::channel::<()>();
        let callback = Closure::once(move || {
            if let Err(_) = sender.send(()) {
                warn!("oneshot send failed");
            }
        });
        request.set_onsuccess(Some(callback.as_ref().unchecked_ref()));
        request.set_onerror(Some(callback.as_ref().unchecked_ref()));
        receiver.await?;
        Ok(match request.result()?.as_f64() {
            Some(v) if v >= 1.0 => true,
            _ => false,
        })
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let tx = self.idb.transaction_with_str(OBJECT_STORE)?;
        let store = tx.object_store(OBJECT_STORE)?;
        let request = store.get(&key.into())?;
        let (sender, receiver) = oneshot::channel::<()>();
        let callback = Closure::once(move || {
            if let Err(_) = sender.send(()) {
                warn!("oneshot send failed");
            }
        });
        request.set_onsuccess(Some(callback.as_ref().unchecked_ref()));
        request.set_onerror(Some(callback.as_ref().unchecked_ref()));
        receiver.await?;
        Ok(match request.result()? {
            v if v.is_undefined() => None,
            v => Some(js_sys::Uint8Array::new(&v).to_vec()),
        })
    }
}

struct ReadTransaction {
    tx: IdbTransaction,
    store: IdbObjectStore,
}

impl ReadTransaction {
    fn new(idb: &IdbStore) -> Result<ReadTransaction> {
        let tx = idb.idb.transaction_with_str(OBJECT_STORE)?;
        Ok(ReadTransaction {
            store: tx.object_store(OBJECT_STORE)?,
            tx: tx,
        })
    }
}

#[async_trait(?Send)]
impl Read for ReadTransaction {
    async fn has(&self, key: &str) -> Result<bool> {
        let request = self.store.count_with_key(&key.into())?;
        let (sender, receiver) = oneshot::channel::<()>();
        let callback = Closure::once(move || {
            if let Err(_) = sender.send(()) {
                warn!("oneshot send failed");
            }
        });
        request.set_onsuccess(Some(callback.as_ref().unchecked_ref()));
        request.set_onerror(Some(callback.as_ref().unchecked_ref()));
        receiver.await?;
        Ok(match request.result()?.as_f64() {
            Some(v) if v >= 1.0 => true,
            _ => false,
        })
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let request = self.store.get(&key.into())?;
        let (sender, receiver) = oneshot::channel::<()>();
        let callback = Closure::once(move || {
            if let Err(_) = sender.send(()) {
                warn!("oneshot send failed");
            }
        });
        request.set_onsuccess(Some(callback.as_ref().unchecked_ref()));
        request.set_onerror(Some(callback.as_ref().unchecked_ref()));
        receiver.await?;
        Ok(match request.result()? {
            v if v.is_undefined() => None,
            v => Some(js_sys::Uint8Array::new(&v).to_vec()),
        })
    }
}

#[derive(PartialEq, Eq, Debug)]
enum WriteState {
    Open,
    Committed,
    Aborted,
    Errored,
}

struct WriteTransaction {
    rt: ReadTransaction,
    pending: Mutex<HashMap<String, Vec<u8>>>,
    pair: Arc<(Mutex<WriteState>, Condvar)>,
    callbacks: Vec<Closure<dyn FnMut()>>,
}

impl WriteTransaction {
    fn new(idb: &IdbStore) -> Result<WriteTransaction> {
        let tx = idb
            .idb
            .transaction_with_str_and_mode(OBJECT_STORE, web_sys::IdbTransactionMode::Readwrite)?;
        let mut wt = WriteTransaction {
            rt: ReadTransaction {
                store: tx.object_store(OBJECT_STORE)?,
                tx: tx,
            },
            pair: Arc::new((Mutex::new(WriteState::Open), Condvar::new())),
            pending: Mutex::new(HashMap::new()),
            callbacks: Vec::with_capacity(3),
        };

        let tx = &wt.rt.tx;
        let callback = wt.tx_callback(WriteState::Committed);
        tx.set_oncomplete(Some(callback.as_ref().unchecked_ref()));
        wt.callbacks.push(callback);

        let callback = wt.tx_callback(WriteState::Aborted);
        tx.set_onabort(Some(callback.as_ref().unchecked_ref()));
        wt.callbacks.push(callback);

        let callback = wt.tx_callback(WriteState::Errored);
        tx.set_onerror(Some(callback.as_ref().unchecked_ref()));
        wt.callbacks.push(callback);

        Ok(wt)
    }

    fn tx_callback(&self, new_state: WriteState) -> Closure<dyn FnMut()> {
        let pair = self.pair.clone();
        Closure::once(move || {
            spawn_local(async move {
                let (lock, cv) = &*pair;
                let mut state = lock.lock().await;
                *state = new_state;
                cv.notify_one();
            });
        })
    }
}

#[async_trait(?Send)]
impl Read for WriteTransaction {
    async fn has(&self, key: &str) -> Result<bool> {
        match self.pending.lock().await.contains_key(key) {
            true => Ok(true),
            false => self.rt.has(key).await,
        }
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self.pending.lock().await.get(key) {
            Some(v) => Ok(Some(v.to_vec())),
            None => self.rt.get(key).await,
        }
    }
}

#[async_trait(?Send)]
impl Write for WriteTransaction {
    fn as_read<'a>(&'a self) -> &'a dyn Read {
        self
    }

    async fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        self.pending
            .lock()
            .await
            .insert(key.to_string(), value.to_vec());
        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<()> {
        // Define rollback() to succeed if no writes have occurred, even if
        // the underlying transaction has exited. Users who expose themselves
        // to this would notice if they performed any reads after exposing
        // themselves to a situation where the transaction would autocommit.
        let pending = self.pending.lock().await;
        if pending.is_empty() {
            return Ok(());
        }

        let store = self.rt.tx.object_store(OBJECT_STORE)?;
        let mut callbacks = Vec::with_capacity(pending.len());
        let mut puts: Vec<oneshot::Receiver<()>> = Vec::with_capacity(pending.len());
        for (key, value) in pending.iter() {
            let request = store.put_with_key(&js_sys::Uint8Array::from(&value[..]), &key.into())?;
            let (sender, receiver) = oneshot::channel::<()>();
            let callback = Closure::once(move || {
                if let Err(_) = sender.send(()) {
                    warn!("oneshot send failed");
                }
            });
            request.set_onsuccess(Some(callback.as_ref().unchecked_ref()));
            callbacks.push(callback);
            puts.push(receiver);
        }
        join_all(puts).await;

        let (lock, cv) = &*self.pair;
        let state = cv
            .wait_until(lock.lock().await, |state| *state != WriteState::Open)
            .await;
        match self.rt.tx.error() {
            Some(e) => return Err(format!("{:?}", e).into()),
            _ => (),
        }
        if *state != WriteState::Committed {
            return Err(StoreError::Str("Transaction aborted".into()));
        }
        Ok(())
    }

    async fn rollback(self: Box<Self>) -> Result<()> {
        // Define rollback() to succeed if no writes have occurred, even if
        // the underlying transaction has exited.
        if self.pending.lock().await.is_empty() {
            return Ok(());
        }

        let (lock, cv) = &*self.pair;
        match *lock.lock().await {
            WriteState::Committed | WriteState::Aborted => return Ok(()),
            _ => (),
        }

        self.rt.tx.abort()?;
        let state = cv
            .wait_until(lock.lock().await, |state| *state != WriteState::Open)
            .await;
        match self.rt.tx.error() {
            Some(e) => return Err(format!("{:?}", e).into()),
            _ => (),
        }
        if *state != WriteState::Aborted {
            return Err(StoreError::Str("Transaction abort failed".into()));
        }
        Ok(())
    }
}
