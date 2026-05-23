// growable linear memory
//
// ┌─────────────────────────────────────────────────────────┐
// │ 0x00 │ 0x01 │ ... │ len │ ... │ cap │                   │
// └─────────────────────────────────────────────────────────┘
//   └───────────────────┘     └───────────┘
//         used              reserved (can grow)
//
// GC heap (separate, mark-sweep):
//   each object: [mark_byte: u8][type_tag: u8][payload...]
//                 ^-- header_addr       ^-- payload_ptr (returned)

pub struct Memory {
    data: Vec<u8>,
    bump: usize,
    free_list: Vec<(usize, usize)>, // (addr, size)
    // GC heap
    gc_heap: Vec<u8>,
    gc_bump: usize,
    pub(crate) gc_free_list: Vec<(usize, usize)>,
    gc_threshold: usize,
    pub(crate) gc_allocations: Vec<(usize, usize)>, // (header_addr, total_size)
}

const DEFAULT_GC_THRESHOLD: usize = 65536;

impl Memory {
    pub fn new(initial_size: usize) -> Self {
        Memory {
            data: vec![0; initial_size],
            bump: 0,
            free_list: Vec::new(),
            gc_heap: Vec::new(),
            gc_bump: 0,
            gc_free_list: Vec::new(),
            gc_threshold: DEFAULT_GC_THRESHOLD,
            gc_allocations: Vec::new(),
        }
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    pub fn grow(&mut self, bytes: usize) -> usize {
        let old = self.data.len();
        self.data.resize(old + bytes, 0);
        old
    }

    // ── GC heap ─────────────────────────────────

    pub fn needs_gc(&self) -> bool {
        self.gc_bump > self.gc_threshold
    }

    pub fn alloc_gc(&mut self, type_tag: u8, size: usize, roots: &[usize]) -> usize {
        if size == 0 {
            return 0;
        }
        if self.gc_bump > self.gc_threshold && !roots.is_empty() {
            self.collect_gc(roots);
        }
        let total = size + 2;
        if let Some(pos) = self
            .gc_free_list
            .iter()
            .position(|&(_, block_size)| block_size >= total)
        {
            let (addr, block_size) = self.gc_free_list.swap_remove(pos);
            if block_size > total {
                self.gc_free_list.push((addr + total, block_size - total));
            }
            self.gc_heap[addr] = 0;
            self.gc_heap[addr + 1] = type_tag;
            self.gc_allocations.push((addr, total));
            return addr + 2;
        }
        let header = self.gc_bump;
        let end = header + total;
        if end > self.gc_heap.len() {
            self.gc_heap.resize(end, 0);
        }
        self.gc_heap[header] = 0;
        self.gc_heap[header + 1] = type_tag;
        self.gc_bump = end;
        self.gc_allocations.push((header, total));
        header + 2
    }

    pub fn mark(&mut self, ptr: usize) {
        if ptr < 2 {
            return;
        }
        let header = ptr - 2;
        if header + 1 >= self.gc_heap.len() {
            return;
        }
        if self.gc_heap[header] == 1 {
            return;
        }
        self.gc_heap[header] = 1;
    }

    pub fn sweep(&mut self) {
        let mut surviving = Vec::new();
        for &(header, total) in &self.gc_allocations {
            if header + 1 < self.gc_heap.len() && self.gc_heap[header] == 1 {
                self.gc_heap[header] = 0;
                surviving.push((header, total));
            } else {
                self.gc_free_list.push((header, total));
            }
        }
        self.gc_allocations = surviving;
    }

    pub fn collect_gc(&mut self, roots: &[usize]) {
        for &root in roots {
            self.mark(root);
        }
        self.sweep();
        let next = self.gc_bump + 4096;
        if next > self.gc_threshold {
            self.gc_threshold = next;
        }
    }

    pub fn gc_type_tag(&self, payload_ptr: usize) -> u8 {
        if payload_ptr < 2 || payload_ptr - 1 >= self.gc_heap.len() {
            return 0;
        }
        self.gc_heap[payload_ptr - 1]
    }

    pub fn gc_is_marked(&self, payload_ptr: usize) -> bool {
        if payload_ptr < 2 || payload_ptr - 2 >= self.gc_heap.len() {
            return false;
        }
        self.gc_heap[payload_ptr - 2] == 1
    }

    // ── non-GC allocator ────────────────────────

    pub fn alloc(&mut self, size: usize) -> usize {
        // Try to find a free block that fits
        if let Some(pos) = self.free_list.iter().position(|&(_, block_size)| block_size >= size) {
            let (addr, block_size) = self.free_list.swap_remove(pos);
            if block_size > size {
                // Split: put remainder back
                self.free_list.push((addr + size, block_size - size));
            }
            return addr;
        }
        // Fall back to bump allocation
        let end = self.bump + size;
        if end > self.data.len() {
            self.grow(end - self.data.len());
        }
        let ptr = self.bump;
        self.bump = end;
        ptr
    }

    pub fn free(&mut self, ptr: usize, size: usize) {
        self.free_list.push((ptr, size));
    }

    pub fn reset(&mut self) {
        self.bump = 0;
        self.free_list.clear();
        self.gc_bump = 0;
        self.gc_free_list.clear();
        self.gc_allocations.clear();
    }

    pub fn bump_ptr(&self) -> usize {
        self.bump
    }

    // ─────────────────────────────────────────
    // generic access (unsafe, no bounds check)
    // ─────────────────────────────────────────

    /// Reads a plain-old-data value from a byte address without checking bounds.
    ///
    /// # Safety
    ///
    /// The caller must ensure `addr..addr + size_of::<T>()` is inside the memory
    /// allocation. The returned bytes must be a valid bit pattern for `T`.
    #[inline]
    pub unsafe fn read<T: Copy>(&self, addr: usize) -> T {
        unsafe { (self.data.as_ptr().add(addr) as *const T).read_unaligned() }
    }

    /// Writes a plain-old-data value to a byte address without checking bounds.
    ///
    /// # Safety
    ///
    /// The caller must ensure `addr..addr + size_of::<T>()` is inside the memory
    /// allocation.
    #[inline]
    pub unsafe fn write<T: Copy>(&mut self, addr: usize, val: T) {
        unsafe { (self.data.as_mut_ptr().add(addr) as *mut T).write_unaligned(val) };
    }

    // ─────────────────────────────────────────
    // checked access (safe, with bounds check)
    // ─────────────────────────────────────────

    pub fn read_checked<T: Copy>(&self, addr: usize) -> Option<T> {
        if addr + size_of::<T>() <= self.data.len() {
            // SAFETY: bounds check above guarantees the read is within allocation
            Some(unsafe { self.read(addr) })
        } else {
            None
        }
    }

    pub fn write_checked<T: Copy>(&mut self, addr: usize, val: T) -> bool {
        if addr + size_of::<T>() <= self.data.len() {
            // SAFETY: bounds check above guarantees the write is within allocation
            unsafe { self.write(addr, val) };
            true
        } else {
            false
        }
    }

    // ─────────────────────────────────────────
    // raw access
    // ─────────────────────────────────────────

    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let mem = Memory::new(1024);
        assert_eq!(mem.size(), 1024);
    }

    #[test]
    fn test_grow() {
        let mut mem = Memory::new(1024);
        let old = mem.grow(512);
        assert_eq!(old, 1024);
        assert_eq!(mem.size(), 1536);
    }

    #[test]
    fn test_read_write_i64() {
        let mut mem = Memory::new(64);
        unsafe {
            mem.write::<i64>(0, -42);
            assert_eq!(mem.read::<i64>(0), -42);
        }
    }

    #[test]
    fn test_read_write_f64() {
        let mut mem = Memory::new(64);
        unsafe {
            mem.write::<f64>(0, 1.25);
            assert_eq!(mem.read::<f64>(0), 1.25);
        }
    }

    #[test]
    fn test_multiple_values() {
        let mut mem = Memory::new(64);
        unsafe {
            mem.write::<i64>(0, 111);
            mem.write::<i64>(8, 222);
            mem.write::<i32>(16, 333);
            mem.write::<i16>(20, 444);
            mem.write::<i8>(22, 55);

            assert_eq!(mem.read::<i64>(0), 111);
            assert_eq!(mem.read::<i64>(8), 222);
            assert_eq!(mem.read::<i32>(16), 333);
            assert_eq!(mem.read::<i16>(20), 444);
            assert_eq!(mem.read::<i8>(22), 55);
        }
    }

    #[test]
    fn test_checked_read() {
        let mem = Memory::new(8);
        assert!(mem.read_checked::<i64>(0).is_some());
        assert!(mem.read_checked::<i64>(1).is_none()); // out of bounds
    }

    #[test]
    fn test_checked_write() {
        let mut mem = Memory::new(8);
        assert!(mem.write_checked::<i64>(0, 42));
        assert!(!mem.write_checked::<i64>(1, 42)); // out of bounds
    }

    #[test]
    fn gc_mark_sweep_collects_dead_objects() {
        let mut mem = Memory::new(64);
        let obj1 = mem.alloc_gc(1, 16, &[]);
        let obj2 = mem.alloc_gc(2, 32, &[]);
        let obj3 = mem.alloc_gc(3, 64, &[]);

        assert_eq!(mem.gc_type_tag(obj1), 1);
        assert_eq!(mem.gc_type_tag(obj2), 2);
        assert_eq!(mem.gc_type_tag(obj3), 3);
        assert!(!mem.gc_is_marked(obj1));
        assert!(!mem.gc_is_marked(obj2));
        assert!(!mem.gc_is_marked(obj3));

        let alloc_count_before = mem.gc_allocations.len();
        assert_eq!(alloc_count_before, 3);

        mem.collect_gc(&[obj2]);

        assert!(!mem.gc_is_marked(obj2));
        assert_eq!(mem.gc_allocations.len(), 1);
        assert_eq!(mem.gc_free_list.len(), 2);
    }

    #[test]
    fn gc_free_list_reuse() {
        let mut mem = Memory::new(64);
        let _obj1 = mem.alloc_gc(1, 16, &[]);
        let obj2 = mem.alloc_gc(2, 16, &[]);
        mem.collect_gc(&[obj2]);
        let obj3 = mem.alloc_gc(3, 16, &[]);
        assert_eq!(mem.gc_type_tag(obj3), 3);
        assert_eq!(mem.gc_allocations.len(), 2);
    }
}
