//! A test over a list of files, run as eight tests of its own so that nextest runs them at once.
//!
//! One process runs each test, so a test that walks the whole corpus sets the length of the suite
//! however many cores are idle; its shards take every eighth file each.

/// How many tests a sharded test is split into; `shard_tests!` writes this many.
pub(crate) const SHARDS: usize = 8;

/// Every `SHARDS`th item of `items` from `index`, so the shards are even whatever order the list is in.
pub(crate) fn take<T>(items: Vec<T>, index: usize) -> Vec<T> {
    items
        .into_iter()
        .enumerate()
        .filter(|(at, _)| at % SHARDS == index)
        .map(|(_, item)| item)
        .collect()
}

/// Writes `shard_0` to `shard_7`, each calling `$run` with its index; put it in a module named for the test.
macro_rules! shard_tests {
    ($run:path) => {
        #[test]
        fn shard_0() {
            $run(0)
        }
        #[test]
        fn shard_1() {
            $run(1)
        }
        #[test]
        fn shard_2() {
            $run(2)
        }
        #[test]
        fn shard_3() {
            $run(3)
        }
        #[test]
        fn shard_4() {
            $run(4)
        }
        #[test]
        fn shard_5() {
            $run(5)
        }
        #[test]
        fn shard_6() {
            $run(6)
        }
        #[test]
        fn shard_7() {
            $run(7)
        }
    };
}
pub(crate) use shard_tests;
