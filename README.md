<p align="center">
  <img src="images/nabla.svg" alt="Nabla logo" width="150">
</p>

<h1 align="center">$\nabla$</h1>

Experimenting and learning about lazy functional language implementation from
first principles, with inspiration from Miranda, Haskell, Rust, and various
literatures from SPJ, Wolfe, and Banerjee.

Planned features & non-features:
- no support for type classes for now (eg: "System F"-like core for now)
- GC'd backend runtime, with G-machine implementation, to be done in Rust
- evaluation strategy: weak head normal form
- inferred types (HW algo) and optional user specified types
- no support for list comprehension for now
- not optimizing for efficient compilation for now
- support for literal (range) pattern in case expression
- monomorphization [planned]
- builtin tabular(arrays/matrices) support [possibly/experimental]
- experimental construct for explicit finite iterations (eg: like recursion and loops but with termination guarantee) + strictness opt-in => enabling polyhedral analysis [possibly/experimental]
