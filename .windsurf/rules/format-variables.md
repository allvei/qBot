---
trigger: always_on
---

Rust's format macros allows variables to be used within the string.

Instead of:
format!("• Admin: {}, display")

We can do:
format!("• Admin: {display}")