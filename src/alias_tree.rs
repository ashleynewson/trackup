use std::vec::Vec;

pub struct AliasTree<T> {
    // size: usize,
    levels: Vec<Vec<T>>,
}

impl<T: Clone + std::ops::BitOr<Output=T> + std::ops::BitAnd<Output=T> + std::cmp::PartialEq> AliasTree<T> {
    pub fn new(size: usize, init: T) -> Self {
        let mut levels: Vec<Vec<T>> = Vec::new();

        let mut level_size: usize = size;
        let mut level_init = init;

        while level_size > 0 {
            let mut level = Vec::with_capacity(level_size);
            for _ in 0..level_size {
                level.push(level_init.clone());
            }
            levels.push(level);
            level_init = level_init.clone() | level_init.clone();
            level_size /= 2;
        }

        AliasTree{
            // size,
            levels,
        }
    }

    fn merge_up(&mut self, index: usize) {
        let mut index = index;
        for l in 1..self.levels.len() {
            let coarse_index = index / 2;
            if coarse_index >= self.levels[l].len() {
                return;
            }
            let merged = self.levels[l-1][index].clone() | self.levels[l-1][index ^ 1].clone();
            if merged == self.levels[l][coarse_index] {
                // Parent already has mask.
                break;
            }
            self.levels[l][coarse_index] = merged;
            index = coarse_index;
        }
    }

    #[allow(dead_code)]
    pub fn get(&self, index: usize) -> &T {
        &self.levels[0][index]
    }

    pub fn set(&mut self, index: usize, value: T) {
        self.levels[0][index] = value;
        self.merge_up(index);
    }

    pub fn or_mask(&mut self, index: usize, value: T) -> &T {
        self.levels[0][index] = self.levels[0][index].clone() | value;
        self.merge_up(index);
        &self.levels[0][index]
    }

    #[allow(dead_code)]
    pub fn and_mask(&mut self, index: usize, value: T) -> &T {
        self.levels[0][index] = self.levels[0][index].clone() & value;
        self.merge_up(index);
        &self.levels[0][index]
    }

    // height = 0 is full detail.
    fn to_level_and_index(&self, index: usize, height: usize) -> (usize, usize) {
        let mut index = index;
        let mut accepted_level = 0;
        for l in 1..(height+1) {
            let coarse_index = index / 2;
            if l >= self.levels.len() || coarse_index >= self.levels[l].len() {
                break;
            }
            accepted_level = l;
            index = coarse_index;
        }
        (accepted_level, index)
    }

    pub fn get_aliased(&self, index: usize, height: usize) -> &T {
        let (level, coarse_index) = self.to_level_and_index(index, height);
        &self.levels[level][coarse_index]
    }

    pub fn find_next<C: Fn(&T) -> bool>(&self, condition: C, start: usize) -> Option<usize> {
        if start >= self.levels[0].len() {
            return None;
        }

        let mut level: usize = 0;
        let mut index: usize = start;

        if condition(&self.levels[0][index]) {
            // We were already on a match
            return Some(index);
        }

        let mut seeking = true;

        // Find a rough place where the next match exists.
        while seeking {
            if index == self.levels[level].len()-1 {
                // There are no more items in this tree.
                // Find the first spill tree with a match...

                // First (potential) spill tree index
                index = (index+1) * 2;
                loop {
                    if level == 0 {
                        // Run out of spill trees
                        return None;
                    } else {
                        level -= 1;
                    }
                    if index < self.levels[level].len() {
                        // There is a spill tree
                        if condition(&self.levels[level][index]) {
                            // Match in this spill tree
                            seeking = false;
                            break;
                        } else {
                            // No match, check next tree
                            index = (index+1) * 2;
                        }
                    } else {
                        // No spill tree, check next level
                        index *= 2;
                    }
                }
            } else if level < self.levels.len() {
                // There should always be a right sibling if this
                // code is reached. (level max always triggers above.)
                if index & 1 == 0 {
                    // was left sibling
                    if condition(&self.levels[level][index|1]) {
                        // right sibling has match
                        index = index | 1;
                        seeking = false;
                    } else {
                        // no match in right child - go up.
                        index /= 2;
                        level += 1;
                    }
                } else {
                    index /= 2;
                    level += 1;
                }
            } else {
                return None;
            }
        }

        // Refine down to the lowest suitable index.
        while level > 0 {
            level -= 1;
            index *= 2;
            if !condition(&self.levels[level][index]) {
                // Left child didn't match, so must be right child
                index = index | 1;
            }
        }

        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_of_2() {
        let mut alias_tree: AliasTree<u8> = AliasTree::new(256, 0);

        assert_eq!(alias_tree.levels.len(), 9);
        assert_eq!(alias_tree.levels[0].len(), 256);
        assert_eq!(alias_tree.levels[1].len(), 128);
        assert_eq!(alias_tree.levels[8].len(), 1);

        assert_eq!(*alias_tree.get(0), 0);
        assert_eq!(*alias_tree.get(1), 0);
        assert_eq!(*alias_tree.get(16), 0);
        assert_eq!(*alias_tree.get(255), 0);

        alias_tree.set(123, 1);
        alias_tree.set(200, 2);

        assert_eq!(*alias_tree.get(100), 0);
        assert_eq!(*alias_tree.get(123), 1);
        assert_eq!(*alias_tree.get(200), 2);

        assert_eq!(*alias_tree.get_aliased(100, 1), 0);
        assert_eq!(*alias_tree.get_aliased(122, 0), 0);
        assert_eq!(*alias_tree.get_aliased(123, 0), 1);
        assert_eq!(*alias_tree.get_aliased(124, 0), 0);
        assert_eq!(*alias_tree.get_aliased(123, 1), 1);
        assert_eq!(*alias_tree.get_aliased(122, 1), 1);
        assert_eq!(*alias_tree.get_aliased(124, 1), 0);
        assert_eq!(*alias_tree.get_aliased(120, 1), 0);
        assert_eq!(*alias_tree.get_aliased(120, 2), 1);
        assert_eq!(*alias_tree.get_aliased(89, 8), 3);
    }

    #[test]
    fn test_power_of_2_minus_1() {
        let mut alias_tree: AliasTree<u8> = AliasTree::new(7, 0);

        for i in 0..7 {
            alias_tree.set(i, 1 << i);
        }
        for i in 0..7 {
            assert_eq!(*alias_tree.get(i), 1 << i);
            assert_eq!(*alias_tree.get_aliased(i, 0), 1 << i);
        }
        assert_eq!(*alias_tree.get_aliased(0, 1), 0x03);
        assert_eq!(*alias_tree.get_aliased(1, 1), 0x03);
        assert_eq!(*alias_tree.get_aliased(2, 1), 0x0c);
        assert_eq!(*alias_tree.get_aliased(3, 1), 0x0c);
        assert_eq!(*alias_tree.get_aliased(4, 1), 0x30);
        assert_eq!(*alias_tree.get_aliased(5, 1), 0x30);
        assert_eq!(*alias_tree.get_aliased(6, 1), 0x40);

        assert_eq!(*alias_tree.get_aliased(0, 2), 0x0f);
        assert_eq!(*alias_tree.get_aliased(1, 2), 0x0f);
        assert_eq!(*alias_tree.get_aliased(2, 2), 0x0f);
        assert_eq!(*alias_tree.get_aliased(3, 2), 0x0f);
        assert_eq!(*alias_tree.get_aliased(4, 2), 0x30);
        assert_eq!(*alias_tree.get_aliased(5, 2), 0x30);
        assert_eq!(*alias_tree.get_aliased(6, 2), 0x40);

        assert_eq!(*alias_tree.get_aliased(0, 3), 0x0f);
        assert_eq!(*alias_tree.get_aliased(1, 3), 0x0f);
        assert_eq!(*alias_tree.get_aliased(2, 3), 0x0f);
        assert_eq!(*alias_tree.get_aliased(3, 3), 0x0f);
        assert_eq!(*alias_tree.get_aliased(4, 3), 0x30);
        assert_eq!(*alias_tree.get_aliased(5, 3), 0x30);
        assert_eq!(*alias_tree.get_aliased(6, 3), 0x40);
    }

    #[test]
    fn test_all_seek() {
        let mut alias_tree: AliasTree<u8> = AliasTree::new(7, 0);
        for i in 0..7 {
            assert_eq!(alias_tree.find_next(|x|{*x!=0}, i), None);
        }        
        for i in 0..7 {
            alias_tree.set(i, 1);
        }
        for i in 0..7 {
            assert_eq!(alias_tree.find_next(|x|{*x!=0}, i), Some(i));
        }        
    }
    #[test]
    fn test_odd_seek() {
        let mut alias_tree: AliasTree<u8> = AliasTree::new(7, 0);
        for i in 0..7 {
            if i&1 == 1 {
                alias_tree.set(i, 1);
            }
        }
        for i in 0..6 {
            let expected = i|1;
            assert_eq!(alias_tree.find_next(|x|{*x!=0}, i), Some(expected));
        }
        assert_eq!(alias_tree.find_next(|x|{*x!=0}, 6), None);
    }
    #[test]
    fn test_even_seek() {
        let mut alias_tree: AliasTree<u8> = AliasTree::new(7, 0);
        for i in 0..7 {
            if i&1 == 0 {
                alias_tree.set(i, 1);
            }
        }
        for i in 0..7 {
            let expected = (i+1)&6;
            assert_eq!(alias_tree.find_next(|x|{*x!=0}, i), Some(expected));
        }
    }
    #[test]
    fn test_single_seek() {
        for i in 0..7 {
            let mut alias_tree: AliasTree<u8> = AliasTree::new(7, 0);
            alias_tree.set(i, 1);
            for j in 0..7 {
                let expected = if i >= j {
                    Some(i)
                } else {
                    None
                };
                assert_eq!(alias_tree.find_next(|x|{*x!=0}, j), expected);
            }
        }
    }
    #[test]
    fn test_no_spill_seek() {
        let mut alias_tree: AliasTree<u8> = AliasTree::new(8, 0);
        assert_eq!(alias_tree.find_next(|x|{*x!=0}, 0), None);
    }
}
