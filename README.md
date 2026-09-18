<p align="center">
  <img src="images/nabla.svg" alt="Nabla logo" width="150">
</p>

<h1 align="center">$\nabla$</h1>

Experimenting and learning about lazy functional language implementation from
first principles, with inspiration from Miranda, Haskell, Rust, and various
literatures from SPJ, Wolfe, and Banerjee.

Planned features & non-features:
- no support for type classes for now (eg: have "System F"-like core)
- GC'd backend runtime, with G-machine implementation, to be done in Rust
- evaluation strategy: weak head normal form
- inferred types (HM algo.) and optional user specified types
- not optimizing for efficient compilation for now
- simple forward sequenced compile pipeline for now
- no builtin support for list for now
- basic builtin ops have these hardcoded precedence and associativity for now
- support for literal (range) pattern in case expression
- monomorphization [planned]
- builtin tabular(arrays/matrices) support [possibly/experimental]
- experimental construct for explicit finite iterations (eg: like recursion and loops but with termination guarantee) on tabular data + strictness opt-in => enabling polyhedral analysis [possibly/experimental]

## Remaining Todo

- code generation
- runtime support

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

isOdd n = case n of
  0 -> Bool.False
  _ -> isEven (n - 1)
```
