<p align="center">
  <img src="images/nabla.svg" alt="Nabla logo" width="150">
</p>

<h1 align="center">$\nabla$</h1>

Experimenting and learning about lazy functional language implementation from
first principles, with inspiration from Miranda, Haskell, Rust, and various
literatures from SPJ (1987, 1992), Wolfe (1995), and Banerjee (1993).
A secondary goal would be to make it usable as an embeddable language in Rust.

Planned features & non-features:
- simple forward sequenced compile pipeline for now
- not optimizing for efficient compilation for now
- no support for type classes for now (eg: have "System F"-like core)
- no support for polymorphic recursion
- inferred types, optional user specified types
- value level variable shadowing
- scoped type variable
- basic builtin ops have hardcoded precedence and associativity for now
- user opt-in strictness [possibly/experimental]
- builtin tabular(arrays/matrices) support to enable tile-based computation [possibly/experimental]
- experimental construct for explicit finite iterations (eg: like recursion and loops but with termination guarantee) on tabular data + strictness opt-in => enabling polyhedral analysis [possibly/experimental]

## Remaining Todo

- middle-end optimizations on functional Core IR
- STG IR: [some notes on operational semantics](http://github.com/hypersimplex/notes_stg/blob/main/notebook.pdf), [notes on design of STG runtime for this project](http://github.com/hypersimplex/nabla/blob/main/doc/stg.pdf)
- code generation
  - instructions for VM (focus on ease of debugging and instrumentation)
  - native instruction code gen for some parts of the program (eg: convert to CFG and generate via Cranelift, or code gen via custom dialect lowering in Pliron/MLIR framework)
- runtime support
  - VM implementation in Rust
  - hybrid: VM + native instructions

## Sample Snippets

```
data Point {
  x :: i64,
  y :: i64,
}

data Circle {
  center :: Point,
  radius :: i64,
}

classify circle = case circle of
  Circle { center = Point { x = 0, y = 0 }, radius = 1..10 } -> "Small and centered at (0, 0)"
  Circle { center = Point { x = cx, y = cy }, radius = r } | r > 100 && cx == 10 && cy == 10 -> "Large and centered at (10, 10)"
  _                                                           -> "Other"

// built-in ADTs (such as Maybe, Bool, Unit/*currently doesn't support `()`*/) are predefined
// data Maybe T = Just T | Nothing

scaleCircle factor circle =
  let f :: i64 = factor
  in case circle of
    Circle { center, radius } -> case radius of
      r | r > 0 -> Maybe.Just (Circle { center = center, radius = r * f })
      _         -> Maybe.Nothing

// data Bool = True | False

isEven n = case n of
  0 -> Bool.True
  _ -> isOdd (n - 1)

isOdd (n :: i64) = case n of
  0 -> Bool.False
  _ -> isEven (n - 1)

add x y = x + y

addTen = add 10.0

// explicit annotation
makeAdder :: i64 -> i64 -> i64
makeAdder base =
  let adder x = add base x
  in adder
```
