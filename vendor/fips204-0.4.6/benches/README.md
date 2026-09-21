Figure-of-merit only; no particular care has been taken to disable turbo-boost etc.
Note that constant-time restrictions on the implementation do impact performance.

Additional performance optimizations are on the roadmap. Near-obvious uplift can be 
had with more careful modular multiplication & addition using fewer reductions. Also, 
'u16' arithmetic has an x86 performance penalty. The `cap_a_hat` pre-compute can be
put into both PublicKey and PrivateKey structs, but current causes stack overflows on
Windows with unoptimized dev builds...this will be investigated further.

~~~
October 15, 2024
Intel® Core™ i7-7700K CPU @ 4.20GHz × 8 Circa 2017 w/ Rust 1.81.0

$ RUSTFLAGS="-C target-cpu=native" cargo bench

ml_dsa_44 keygen        time:   [104.79 µs 104.89 µs 105.02 µs]
ml_dsa_65 keygen        time:   [194.61 µs 194.80 µs 195.10 µs]
ml_dsa_87 keygen        time:   [289.87 µs 290.24 µs 290.87 µs]

ml_dsa_44 sk sign       time:   [223.90 µs 226.32 µs 228.68 µs]
ml_dsa_65 sk sign       time:   [348.26 µs 352.89 µs 357.44 µs]
ml_dsa_87 sk sign       time:   [379.41 µs 385.05 µs 390.63 µs]

ml_dsa_44 pk verify     time:   [20.988 µs 21.016 µs 21.058 µs]
ml_dsa_65 pk verify     time:   [27.951 µs 27.996 µs 28.066 µs]
ml_dsa_87 pk verify     time:   [36.424 µs 36.468 µs 36.547 µs]
~~~