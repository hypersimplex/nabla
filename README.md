<p align="center">
  <img src="images/nabla.svg" alt="Nabla logo" width="150">
</p>

<h1 align="center">$\nabla$</h1>

Experimenting and learning about lazy functional language implementation from
first principles, with inspiration from Miranda, Haskell, Rust, and various
literatures from SPJ (1987, 1992), Wolfe (1995), and Banerjee (1993).

Planned features & non-features:
- simple forward sequenced compile pipeline for now
- not optimizing for efficient compilation for now
- no support for type classes for now (eg: have "System F"-like core)
- STG for backend
- evaluation strategy: weak head normal form
- inferred types (HM algo.) and optional user specified types
- basic builtin ops have hardcoded precedence and associativity for now
- support for infix list notation [todo]
- builtin tabular(arrays/matrices) support [possibly/experimental]
- experimental construct for explicit finite iterations (eg: like recursion and loops but with termination guarantee) on tabular data + strictness opt-in => enabling polyhedral analysis [possibly/experimental]

## Remaining Todo

- STG IR
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

add x y = x + y

addTen = add 10.0

// explicit annotation
makeAdder :: i64 -> i64 -> i64
makeAdder base =
  let adder x = add base x
  in adder
```
