Figure-of-merit only; no particular care has been taken to disable turbo-boost etc.
Note that constant-time restrictions on the implementation do impact performance.

Additional performance optimizations will follow the next update to FIPS 205.

~~~
October 3, 2024
Intel® Core™ i7-7700K CPU @ 4.20GHz × 8 Circa 2017 w/ Rust 1.81.0

$ RUSTFLAGS="-C target-cpu=native" cargo bench

sha2_128f  keygen       time:   [1.7823 ms 1.7830 ms 1.7839 ms]
sha2_192f  keygen       time:   [2.6234 ms 2.6256 ms 2.6279 ms]
sha2_256f  keygen       time:   [6.8753 ms 6.8797 ms 6.8858 ms]
shake_128f keygen       time:   [2.7946 ms 2.7953 ms 2.7961 ms]
shake_192f keygen       time:   [4.0918 ms 4.0954 ms 4.0993 ms]
shake_256f keygen       time:   [10.704 ms 10.717 ms 10.739 ms]
sha2_128s  keygen       time:   [113.89 ms 113.90 ms 113.92 ms]
sha2_192s  keygen       time:   [166.62 ms 166.63 ms 166.65 ms]
sha2_256s  keygen       time:   [109.25 ms 109.34 ms 109.43 ms]
shake_128s keygen       time:   [178.32 ms 178.41 ms 178.52 ms]
shake_192s keygen       time:   [261.50 ms 261.55 ms 261.63 ms]
shake_256s keygen       time:   [173.21 ms 173.22 ms 173.23 ms]

sha2_128f  sign         time:   [41.623 ms 41.635 ms 41.654 ms]
sha2_192f  sign         time:   [68.686 ms 68.886 ms 69.138 ms]
sha2_256f  sign         time:   [141.52 ms 141.54 ms 141.56 ms]
shake_128f sign         time:   [65.349 ms 65.364 ms 65.381 ms]
shake_192f sign         time:   [106.67 ms 106.68 ms 106.70 ms]
shake_256f sign         time:   [217.07 ms 217.25 ms 217.45 ms]
sha2_128s  sign         time:   [867.34 ms 868.15 ms 869.14 ms]
sha2_192s  sign         time:   [1.5404 s 1.5414 s 1.5426 s]
sha2_256s  sign         time:   [1.3559 s 1.3563 s 1.3568 s]
shake_128s sign         time:   [1.3682 s 1.3730 s 1.3788 s]
shake_192s sign         time:   [2.3982 s 2.4085 s 2.4198 s]
shake_256s sign         time:   [2.0949 s 2.1288 s 2.1678 s]

sha2_128f  verify       time:   [2.5693 ms 2.5735 ms 2.5781 ms]
sha2_192f  verify       time:   [3.8974 ms 3.9857 ms 4.0836 ms]
sha2_256f  verify       time:   [3.8619 ms 3.8760 ms 3.8925 ms]
shake_128f verify       time:   [3.9791 ms 4.0046 ms 4.0349 ms]
shake_192f verify       time:   [5.7540 ms 5.7838 ms 5.8202 ms]
shake_256f verify       time:   [6.1739 ms 6.3059 ms 6.4543 ms]
sha2_128s  verify       time:   [887.76 µs 898.88 µs 912.39 µs]
sha2_192s  verify       time:   [1.3260 ms 1.3372 ms 1.3522 ms]
sha2_256s  verify       time:   [1.9167 ms 1.9321 ms 1.9527 ms]
shake_128s verify       time:   [1.3037 ms 1.3070 ms 1.3109 ms]
shake_192s verify       time:   [1.9459 ms 1.9595 ms 1.9802 ms]
shake_256s verify       time:   [2.8772 ms 2.8875 ms 2.9002 ms]
~~~