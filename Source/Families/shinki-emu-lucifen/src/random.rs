/// The legacy subtractive generator backs Lucifen's bytecode random calls.
pub(crate) struct Random {
    table: [i32; 56],
    index: usize,
}

impl Random {
    pub(crate) fn seeded(seed: i32) -> Self {
        let mut random = Self {
            table: [0; 56],
            index: 55,
        };
        random.table[55] = seed;
        let mut previous = seed;
        let mut next = 1;
        for index in 1..55 {
            random.table[(index * 21) % 55] = next;
            (previous, next) = (next, difference(previous, next));
        }
        for _ in 0..3 {
            random.advance();
        }
        random
    }

    pub(crate) fn below(&mut self, limit: u32) -> i32 {
        if limit == 0 {
            return 0;
        }
        self.index += 1;
        if self.index > 55 {
            self.advance();
            self.index = 1;
        }
        (self.table[self.index] as u32 % limit) as i32
    }

    fn advance(&mut self) {
        for index in 1..=24 {
            self.table[index] = difference(self.table[index], self.table[index + 31]);
        }
        for index in 25..=55 {
            self.table[index] = difference(self.table[index], self.table[index - 24]);
        }
    }
}

fn difference(left: i32, right: i32) -> i32 {
    let value = left.wrapping_sub(right);
    if value < 0 {
        value.wrapping_add(1_000_000_000)
    } else {
        value
    }
}
