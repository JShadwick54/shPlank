# Rust Cheatsheet — shPlank fundamentals

A quick reference for the core Rust we covered before Step 2. Anchored to C#/C++
mental models. This is a "remind me how this works" sheet, not a tutorial.

---

## 1. Ownership & moves

- Every value has **exactly one owner** (a variable). When the owner goes out of
  scope (`}`), the value is **freed automatically**. No garbage collector, no manual `free`.
- Assigning or passing a heap-owning value **moves** ownership; the original variable
  becomes invalid.

```rust
let a = String::from("hi");
let b = a;          // ownership MOVES from a to b
// println!("{a}"); // ERROR E0382: value borrowed after move
println!("{b}");    // fine
```

- **`Copy` types** (stack-only: `i32`, `u32`, `bool`, `char`, `f64`, ...) are copied
  instead of moved, so the original stays valid.

```rust
let x = 5;
let y = x;          // COPY, not move
println!("{x} {y}"); // both fine
```

- `.clone()` makes an explicit deep copy when you genuinely need two owners.

```rust
let a = String::from("hi");
let b = a.clone();   // b owns its own copy; a still valid
```

**C++ analogy:** like C++11 move semantics, but the default and compiler-enforced.

---

## 2. Borrowing (`&` and `&mut`)

Borrow = let someone use a value **without taking ownership**.

```rust
fn read(s: &String) { println!("{s}"); }      // immutable borrow (read-only)
fn grow(s: &mut String) { s.push_str("!"); }  // mutable borrow (can modify)

let mut name = String::from("josh");
read(&name);        // lend a read-only reference
grow(&mut name);    // lend a mutable reference (name must be `mut`)
println!("{name}"); // main still owns it
```

**The rule that defines the borrow checker — shared XOR mutable:**
at any moment, for a given value, you may have *either*
- any number of immutable borrows (`&`), **or**
- exactly one mutable borrow (`&mut`),
- **never both at once.**

This is what guarantees **no data races at compile time** ("fearless concurrency").

> A borrow ends at its **last use**, not at the closing `}` (non-lexical lifetimes).

**C# analogy:** `&` ≈ read-only reference; everything-is-a-reference C# gives this implicitly.
**C++ analogy:** `&` ≈ `const T&`, `&mut` ≈ `T&`.

---

## 3. Structs & enums

### Struct — named bundle of fields (like a C# class/record)

```rust
struct Post {
    id: u32,
    title: String,
    body: String,
}

// Behavior goes in a SEPARATE impl block.
impl Post {
    fn summary(&self) -> String {              // &self = borrow the instance to read
        format!("#{} {}", self.id, self.title)
    }
}

let p = Post { id: 1, title: String::from("Hi"), body: String::from("...") };
println!("{}", p.summary());
```

- Every field **must** be initialized — no `null`, no implicit default.
- Method receivers: `&self` (read), `&mut self` (modify), `self` (consume).

### Enum — a value that is **one of several variants**, each can carry data

```rust
enum View {
    Feed,             // no data
    PostDetail(u32),  // carries which post id
}
```

Much more powerful than C# enums (closer to C++ `std::variant` / tagged union).

### `match` — exhaustive branching (compiler forces every variant handled)

```rust
fn describe(v: &View) -> String {
    match v {
        View::Feed => String::from("the feed"),
        View::PostDetail(id) => format!("post #{id}"),
    }
}
```

If you add a variant and forget to handle it, **it won't compile.**

---

## 4. Option & Result — no null, no exceptions

Both are just enums from the standard library.

### `Option<T>` — "a value, or nothing" (replaces null)

```rust
enum Option<T> { Some(T), None }   // built in

fn find_post(posts: &[Post], id: u32) -> Option<&Post> {
    for p in posts {
        if p.id == id { return Some(p); }
    }
    None
}

match find_post(&posts, 2) {
    Some(p) => println!("found {}", p.title),
    None    => println!("not found"),
}
```

### `Result<T, E>` — "success, or an error" (replaces exceptions)

```rust
enum Result<T, E> { Ok(T), Err(E) }   // built in

let n: Result<u32, _> = "42".parse();
match n {
    Ok(v)  => println!("got {v}"),
    Err(e) => println!("failed: {e}"),
}
```

### The `?` operator — propagate errors / None upward

```rust
fn add(a: &str, b: &str) -> Result<u32, Box<dyn std::error::Error>> {
    let x: u32 = a.parse()?;  // if Err, return it from this fn immediately
    let y: u32 = b.parse()?;
    Ok(x + y)
}
```

`?` = "unwrap if Ok/Some, otherwise early-return the Err/None." It's everywhere in
real I/O, database, and network code.

> `{}` = normal display. `{:?}` = debug display (derive with `#[derive(Debug)]` on your structs).

---

## 5. Async (overview)

For tasks that spend most of their time **waiting** (network, disk, keypresses), async
lets a few threads juggle many waiting tasks instead of one thread each.

| Rust            | C# equivalent        | What it is                                  |
|-----------------|----------------------|---------------------------------------------|
| `async fn`      | `async Task<T>`      | function that can pause; returns a `Future` |
| `Future`        | `Task<T>`            | "work not finished yet"                     |
| `.await`        | `await`              | pause until done; let other tasks run       |
| `tokio`         | runtime / threadpool | the engine that actually drives futures     |

```rust
#[tokio::main]                  // starts the runtime; lets main be async
async fn main() {
    something().await;          // pause here without blocking the OS thread
}
```

**Two Rust-specific gotchas:**
1. **Futures are lazy** — calling an `async fn` runs nothing until it's `.await`ed.
   (In C#, calling an async method starts it immediately.)
2. **Rust ships no runtime** — you pick one. `tokio` is the de-facto choice; `russh`
   is built on it.

---

## Reading compiler errors

- Errors have codes: `error[E0382]` → `rustc --explain E0382` for a full writeup.
- The error shows three things: where the value came from, where ownership moved,
  and where you broke the rule. Read top to bottom.
- The Rust compiler is a teacher — its suggestions ("consider cloning", "consider
  borrowing") are usually correct.
