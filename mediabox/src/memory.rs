use std::{sync::mpsc::{Sender, Receiver, self}, mem, fmt, ops::Deref};

pub struct Memory {
    memory: Vec<u8>,
    send: Sender<Vec<u8>>,
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[u8; {}]", self.memory.len())
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        let mut memory = Vec::new();
        mem::swap(&mut self.memory, &mut memory);

        let _ = self.send.send(memory);
    }
}

impl Deref for Memory {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.memory
    }
}

pub struct MemoryPoolConfig {
    max_capacity: Option<usize>,
    default_memory_capacity: usize,
}

pub struct MemoryPool {
    pool: Vec<Vec<u8>>,
    config: MemoryPoolConfig,
    alloc_count: usize,
    recv: Receiver<Vec<u8>>,
    send: Sender<Vec<u8>>,
}

impl MemoryPool {
    pub fn new(config: MemoryPoolConfig) -> Self {
        let (send, recv) = mpsc::channel();

        MemoryPool { pool: Vec::new(), config, alloc_count: 0, recv, send }
    }

    pub fn alloc(&mut self, size: usize) -> Memory {
        if let Some(mem) = self.try_alloc(size) {
            return mem;
        }

        self.pool.push(self.recv.recv().unwrap());

        self.try_alloc(size).unwrap()
    }

    pub fn try_alloc(&mut self, size: usize) -> Option<Memory> {
        while let Ok(mem) = self.recv.try_recv() {
            dbg!("returning memory");
            self.pool.push(mem);
        }

        if let Some(mem) = self.find_best_alloc(size) {
            return Some(self.create_memory(mem));
        }

        if let Some(max_alloc_account) = self.config.max_capacity {
            if let Some(mem) = self.find_best_realloc(size) {
                return Some(self.create_memory(mem));
            }

            if self.alloc_count >= max_alloc_account {
                return None;
            }
        }

        let alloc_size = size.max(self.config.default_memory_capacity);
        let new_memory = vec![0u8; alloc_size];
        self.alloc_count += 1;
        Some(self.create_memory(new_memory))
    }

    fn create_memory(&self, memory: Vec<u8>) -> Memory {
        Memory { memory, send: self.send.clone(), }
    }

    fn find_best_alloc(&mut self, size: usize) -> Option<Vec<u8>> {
        if self.pool.is_empty() {
            return None;
        }

        if let Some(idx) = self.pool.iter().position(|m| size <= m.len()) {
            return Some(self.pool.swap_remove(idx));
        }

        None
    }

    fn find_best_realloc(&mut self, size: usize) -> Option<Vec<u8>> {
        if self.pool.is_empty() {
            return None;
        }

        let mut mem = self.pool.swap_remove(0);

        mem.resize(size, 0u8);

        Some(mem)
    }
}

#[cfg(test)]
mod test {
    use assert_matches::assert_matches;
    use test_case::test_case;

    use super::*;

    #[test]
    fn test_memory_pool_capacity() {
        let config = MemoryPoolConfig { max_capacity: Some(1), default_memory_capacity: 1024 };
        let mut pool = MemoryPool::new(config);

        let first = pool.try_alloc(1024);
        assert!(first.is_some());

        let second = pool.try_alloc(1024);
        assert!(second.is_none());
    }

    #[test]
    fn test_memory_pool_capacity_and_return_memory() {
        let config = MemoryPoolConfig { max_capacity: Some(1), default_memory_capacity: 1024 };
        let mut pool = MemoryPool::new(config);

        let first = pool.try_alloc(1024);
        assert!(first.is_some());

        let second = pool.try_alloc(1024);
        assert!(second.is_none());

        drop(first);

        let third = pool.try_alloc(1024);
        assert!(third.is_some());
    }

    #[test]
    fn test_memory_pool_capacity_and_return_memory_and_alloc_over_default() {
        let config = MemoryPoolConfig { max_capacity: Some(1), default_memory_capacity: 1024 };
        let mut pool = MemoryPool::new(config);

        let first = pool.try_alloc(1024);
        assert_matches!(first, Some(ref mem) => {
            assert_eq!(mem.len(), 1024);
        });

        let second = pool.try_alloc(1024);
        assert!(second.is_none());

        drop(first);

        let third = pool.try_alloc(2048);
        assert_matches!(third, Some(mem) => {
            assert!(mem.len() >= 2048);
        });
    }

    #[test_case(1024)]
    #[test_case(2048)]
    fn test_memory_pool_alloc_over_default(size: usize) {
        let config = MemoryPoolConfig { max_capacity: None, default_memory_capacity: 1024 };
        let mut pool = MemoryPool::new(config);

        let first = pool.try_alloc(size);
        assert_matches!(first, Some(mem) => {
            assert_eq!(mem.len(), size);
        });
    }

    #[test]
    fn test_memory_pool_alloc_under_default() {
        let config = MemoryPoolConfig { max_capacity: None, default_memory_capacity: 1024 };
        let mut pool = MemoryPool::new(config);

        let first = pool.try_alloc(512);
        assert_matches!(first, Some(mem) => {
            assert_eq!(mem.len(), 1024);
        });
    }
}
