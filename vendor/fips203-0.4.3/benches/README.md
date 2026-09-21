Figure-of-merit only; no particular care has been taken to disable turbo-boost etc.
Note that constant-time restrictions on the implementation do impact performance.

Additional performance optimizations will follow ...

~~~
October 12, 2024
Intel® Core™ i7-7700K CPU @ 4.20GHz × 8 Circa 2017 w/ Rust 1.81

$ RUSTFLAGS="-C target-cpu=native" cargo bench

ml_kem_512  KeyGen      time:   [27.694 µs 27.705 µs 27.720 µs]
ml_kem_768  KeyGen      time:   [46.650 µs 46.662 µs 46.677 µs]
ml_kem_1024 KeyGen      time:   [71.232 µs 71.247 µs 71.263 µs]

ml_kem_512  Encaps      time:   [27.878 µs 27.884 µs 27.892 µs]
ml_kem_768  Encaps      time:   [44.768 µs 44.800 µs 44.840 µs]
ml_kem_1024 Encaps      time:   [69.829 µs 69.852 µs 69.878 µs]

ml_kem_512  Decaps      time:   [39.295 µs 39.314 µs 39.334 µs]
ml_kem_768  Decaps      time:   [60.061 µs 60.129 µs 60.211 µs]
ml_kem_1024 Decaps      time:   [85.276 µs 85.386 µs 85.516 µs]
~~~
