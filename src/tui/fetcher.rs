use memmap2::Mmap;
use lru::LruCache;
use serde_json::Value;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

pub struct RecordFetcher {
    mmap: Mmap,
    offsets: Vec<u64>,
    cache: Mutex<LruCache<usize, Value>>,
}

impl RecordFetcher {
    pub fn new(mmap: Mmap, offsets: Vec<u64>) -> Self {
        Self {
            mmap,
            offsets,
            cache: Mutex::new(LruCache::new(NonZeroUsize::new(1000).unwrap())),
        }
    }

    pub fn fetch_record(&self, idx: usize) -> Result<Value, Box<dyn std::error::Error>> {
        // 1. Check LRU Cache
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(val) = cache.get(&idx) {
                return Ok(val.clone());
            }
        }

        // 2. Calculate byte range
        let start = self.offsets[idx] as usize;
        let end = self.offsets.get(idx + 1)
            .map(|&next| next as usize)
            .unwrap_or(self.mmap.len());

        // 3. Extract slice and parse
        let slice = &self.mmap[start..end];
        let val: Value = serde_json::from_slice(slice)?;

        // 4. Update cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.put(idx, val.clone());
        }

        Ok(val)
    }
}
