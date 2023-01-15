use std::sync::mpsc::{Sender, Receiver, self};

pub struct Memory {
    memory: Vec<u8>,
    send: Sender<Vec<u8>>,
}

pub struct MemoryPoolConfig {
    max_capacity: Option<usize>,
    default_memory_capacity: usize,
}

pub struct MemoryPool {
    pool: Vec<Vec<u8>>,
    config: MemoryPoolConfig,
    recv: Receiver<Vec<u8>>,
    send: Sender<Vec<u8>>,
}

impl MemoryPool {
    pub fn new(config: MemoryPoolConfig) -> Self {
        let (send, recv) = mpsc::channel();

        MemoryPool { pool: Vec::new(), config, recv, send }
    }

    pub fn alloc(&mut self, size: u64) -> Memory {
        if let Some(mem) = self.try_alloc(size) {
            return mem;
        }

        self.pool.push(self.recv.recv().unwrap());

        self.try_alloc(size).unwrap()
    }

    pub fn try_alloc(&mut self, size: u64) -> Option<Memory> {
        while let Ok(mem) = self.recv.try_recv() {
            self.pool.push(mem);
        }

        if let Some(mem) = self.find_best_alloc(size) {
            return Some(self.create_memory(mem));
        }

        None
    }

    fn create_memory(&self, memory: Vec<u8>) -> Memory {
        Memory { memory, send: self.send.clone(), }
    }

    fn find_best_alloc(&mut self, size: u64) -> Option<Vec<u8>> {
        if self.pool.is_empty() {
            return None;
        }

        None
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_memory_pool_capacity() {
        let config = MemoryPoolConfig { max_capacity: 1, default_memory_capacity: 1024 };
        let pool = MemoryPool::new(config);

        let first = pool.try_alloc(1024);
        assert!(first.is_some());

        let second = pool.try_alloc(1024);
        assert!(first.is_none());
    }
}
