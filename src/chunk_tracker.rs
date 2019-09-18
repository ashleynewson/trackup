use alias_tree::AliasTree;

pub struct ChunkTracker {
    chunk_count: usize,
    chunks: AliasTree<u8>,
}

const FLAG_UNPROCESSED: u8 = 2;
const FLAG_DIRTY: u8 = 1;

impl ChunkTracker {
    pub fn new(chunk_count: usize) -> Self {
        // let chunk_count = 
        //     devices.sectors / chunk_size
        //     + if devices.sectors % chunk_size {1} else {0};

        let chunks: AliasTree<u8> = AliasTree::new(chunk_count, FLAG_UNPROCESSED);

        ChunkTracker {
            chunk_count,
            chunks,
        }
    }

    // pub fn consume_dirty_queue(&mut self, change_log: &mut ChunkLog) {
    //     while let Some(index) = change_log.consume() {
    //         mark_chunk
    //     }
    // }

    // pub fn sector_to_chunk(&self, sector: usize) {
    //     sector / self.chunk_size;
    // }

    pub fn clear_chunk(&mut self, index: usize) {
        self.chunks.set(index, 0);
    }

    pub fn mark_chunk(&mut self, index: usize) {
        self.chunks.or_mask(index, FLAG_DIRTY);
    }

    #[allow(dead_code)]
    pub fn mark_chunks(&mut self, start: usize, end: usize) {
        let end = if end < self.chunk_count {
            end
        } else {
            self.chunk_count
        };
        for i in start..end {
            self.chunks.or_mask(i, FLAG_DIRTY);
        }
    }

    pub fn find_next(&self, start: usize) -> Option<usize> {
        self.chunks.find_next(|x|{*x!=0}, start)
    }

    pub fn summary_report(&self, height: usize) -> String {
        // BUG: If the chunk count isn't a multiple of 1<<height, the last chunk may not be representative.
        let factor: usize = 1 << height;
        let checks = (self.chunk_count-1)/factor+1;

        let mut diagram: Vec<char> = Vec::with_capacity(checks);
        let mut done = 0;

        for index in 0..checks {
            let flags = *self.chunks.get_aliased(index*factor, height);
            let character: char =
                if flags & (FLAG_UNPROCESSED | FLAG_DIRTY) == (FLAG_UNPROCESSED | FLAG_DIRTY) {
                    ';'
                } else if flags & FLAG_UNPROCESSED == FLAG_UNPROCESSED {
                    '.'
                } else if flags & FLAG_DIRTY == FLAG_DIRTY {
                    ','
                } else {
                    '#'
                };
            diagram.push(character);
            if flags == 0 {
                done += 1;
            }
        }

        let diagram_string: String = diagram.into_iter().collect();

        format!("\nChunk map ({} chunks per cell):\n{}\n\nProgess: {}%\nUnprocessedDirty ;  Unprocessed .  Dirty ,  Done #\n", factor, diagram_string, done * 100 / checks)
    }
}
