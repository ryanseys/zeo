# Conformance failures — full detail

Suite `spinel` — **24 failing test(s)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `69f0c9d`

Every non-passing test with its source path, the reference it is diffed
against, verdict/bucket, captured stderr, and the full expected-vs-actual
diff — enough to understand each failure without re-running the harness.
Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.

## `block_local_fresh_per_iteration` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/block_local_fresh_per_iteration.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/block_local_fresh_per_iteration.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 4
--- expected (7 lines)
["none", 60, "none", 70, "none"]
["small", "big", "small", "big"]
yes
fresh
fresh
["high", "low", "high", "low"]
3

--- actual (7 lines)
["none", 60, "none", 70, "none"]
["small", "big", "small", "big"]
yes
yes
yes
["high", "low", "high", "low"]
3
```

---

## `const_aliased_class_reopen_include` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/const_aliased_class_reopen_include.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/const_aliased_class_reopen_include.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:wordy` (cluster `P`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/const_aliased_class_reopen_include.rb:18:in '<main>': undefined method 'wordy' for an instance of Integer (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
wordy_42
hi from greeter

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
test/const_aliased_class_reopen_include.rb:18:in '<main>': undefined method 'wordy' for an instance of Integer (NoMethodError)
```

---

## `constant_path` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/constant_path.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/constant_path.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-const:M::C` (cluster `P`)

stderr:
```
uninitialized constant M::C (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 14
--- expected (21 lines)
0
0
true
true
4
2
6
a
5
6
true
42
100
7
7
11
11
47
31
start
init

--- actual (13 lines)
0
0
true
true
4
2
6
a
5
6
true
42
100

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uninitialized constant M::C (NameError)
```

---

## `data_new_validation` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/data_new_validation.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/data_new_validation.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 4
--- expected (4 lines)
"argerror"
"argerror"
"argerror"
"argerror"

--- actual (4 lines)
"argerror"
"argerror"
"argerror"
"no error"
```

---

## `ffi_callback` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-e249` (cluster `?`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb:11:in 'L.qsort': cannot convert Proc into an FFI pointer (TypeError)
	from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb:24:in '<main>'
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
[1, 1, 2, 3, 4, 5, 6, 9]
[40, 30, 20, 10]
5
miss
done

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (2 lines)
test/ffi_callback.rb:11:in 'L.qsort': cannot convert Proc into an FFI pointer (TypeError)
	from test/ffi_callback.rb:24:in '<main>'
```

---

## `ffi_variadic` — FAIL_COMPILE

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_variadic.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_variadic.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_COMPILE (stage `compile`) · bucket `auto-territory-not-syntaxerror-ed71` (cluster `?`)

stderr:
```
 14 │     
    ╰────
  help: valid Ruby that zeo does not compile yet (NotImplementedError
        territory, not a SyntaxError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
1-22-hi
8
3.50
[k=7]
plain

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module C (NoMethodError)
```

---

## `frozen_string_literal_per_file` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/frozen_string_literal_per_file.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/frozen_string_literal_per_file.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
true
false
false
can't modify frozen String: "helper buf"
"plain!"

--- actual (5 lines)
false
false
false
BUG: no raise
"plain!"
```

---

## `frozen_string_literal_per_file_rev` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/frozen_string_literal_per_file_rev.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/frozen_string_literal_per_file_rev.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-7011` (cluster `?`)

stderr:
```
/Users/ryanseys/dev/zeo/conformance/corpus/test/frozen_string_literal_per_file/helper_plain.rb:7:in 'Object#plain_helper_build': can't modify frozen String: "plain" (FrozenError)
	from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/frozen_string_literal_per_file_rev.rb:8:in '<main>'
```

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (3 lines)
true
false
"plain!"

--- actual (2 lines)
true
true

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (2 lines)
/Users/ryanseys/dev/zeo/conformance/corpus/test/frozen_string_literal_per_file/helper_plain.rb:7:in 'Object#plain_helper_build': can't modify frozen String: "plain" (FrozenError)
	from test/frozen_string_literal_per_file_rev.rb:8:in '<main>'
```

---

## `hash_each_with_object` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_each_with_object.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_each_with_object.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-2467` (cluster `?`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_each_with_object.rb:15:in 'block in <main>': undefined method '<<' for nil (NoMethodError)
	from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_each_with_object.rb:15:in '<main>'
```

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (2 lines)
[10, 20, 30]
[100, 200, 300]

--- actual (1 lines)
[10, 20, 30]

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (2 lines)
test/hash_each_with_object.rb:15:in 'block in <main>': undefined method '<<' for nil (NoMethodError)
	from test/hash_each_with_object.rb:15:in '<main>'
```

---

## `inline_yield_method_with_return` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/inline_yield_method_with_return.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/inline_yield_method_with_return.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (2 lines)
5
-1

--- actual (2 lines)
5
1
```

---

## `instance_exec_def_singleton` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/instance_exec_def_singleton.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/instance_exec_def_singleton.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-4add` (cluster `?`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/instance_exec_def_singleton.rb:11:in 'block in <main>': undefined method 'define_method' for an instance of Box (NoMethodError)
	from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/instance_exec_def_singleton.rb:11:in '<main>'
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
hi

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (2 lines)
test/instance_exec_def_singleton.rb:11:in 'block in <main>': undefined method 'define_method' for an instance of Box (NoMethodError)
	from test/instance_exec_def_singleton.rb:11:in '<main>'
```

---

## `issue_2975` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_2975.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_2975.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
"k"
5
"sv"
P
true

--- actual (5 lines)
C
-6031675459494645702
S
P
true
```

---

## `issue_3037` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3037.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3037.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
caught NameError
undefined local variable or method 'undefined_bareword_thing' for main
"fallback"
42

--- actual (4 lines)
caught NoMethodError
undefined method 'undefined_bareword_thing' for main
"fallback"
42
```

---

## `issue_3098` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3098.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3098.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
true
false
false

--- actual (3 lines)
false
false
false
```

---

## `issue_3226` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3226.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3226.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 5
--- expected (8 lines)
{a: 1, b: 2}
{a: 1, b: 3}
[:a, :b]
{a: 1, b: 2}
NameError
[1, 2]
2
{a: 1, b: 2}

--- actual (8 lines)
{a: 1, b: 2}
{a: 1, b: 3}
[:a, :b]
{a: 1, b: 2}
NoMethodError
[1, 2]
2
{a: 1, b: 2}
```

---

## `regexp_brace_zero_lower` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_brace_zero_lower.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_brace_zero_lower.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 4
--- expected (16 lines)
"aa"
"123"
"aab"
["aa", "aa", ""]
""
""
"aaaa"
"b"
""
"aaa"
"aaaa"
"aaa"
"aaaa"
"12-34"
""
""

--- actual (16 lines)
"aa"
"123"
"aab"
["aa", "aa"]
""
""
"aaaa"
"b"
""
"aaa"
"aaaa"
"aaa"
"aaaa"
"12-34"
""
""
```

---

## `regexp_gsub_zero_width` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_gsub_zero_width.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_gsub_zero_width.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 6
--- expected (12 lines)
"abc!"
"x!\ny!"
"x!\ny!\nz!"
">a\n>b"
"-h-e-l-l-o-"
"XaXXcX"
"|hello| |world|"
"abc!"
["Name", "Lang", "Year"]
"Ruby"
5
11

--- actual (12 lines)
"abc!"
"x!\ny!"
"x!\ny!\nz!"
">a\n>b"
"-h-e-l-l-o-"
"XaXcX"
"|hello| |world|"
"abc!"
["Name", "Lang", "Year"]
"Ruby"
5
11
```

---

## `regexp_inline_options` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_inline_options.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_inline_options.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 10
--- expected (30 lines)
1
0
nil
0
nil
0
nil
0
nil
0
nil
0
nil
0
0
ws-x: RegexpError
*: RegexpError
+: RegexpError
?: RegexpError
a***: RegexpError
(?~foo): RegexpError
(?(a)b): RegexpError
(?q)a: RegexpError
(?x:ab): compiled
2
nil
1
3
"いうえ"
nil

--- actual (30 lines)
1
0
nil
0
nil
0
nil
0
nil
nil
nil
nil
nil
0
0
ws-x: compiled
*: RegexpError
+: RegexpError
?: RegexpError
a***: compiled
(?~foo): RegexpError
(?(a)b): compiled
(?q)a: RegexpError
(?x:ab): compiled
2
nil
1
3
"いうえ"
nil
```

---

## `regexp_pike_capture_and_bol` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_pike_capture_and_bol.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_pike_capture_and_bol.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 6
--- expected (10 lines)
["1", nil]
[nil, "b", nil]
[nil, nil, "c"]
"baz"
[[nil, "a"], ["1", nil], [nil, "b"], ["2", nil]]
1
2
false
true
["a", "b"]

--- actual (10 lines)
["1", nil]
[nil, "b", nil]
[nil, nil, "c"]
"baz"
[[nil, "a"], ["1", nil], [nil, "b"], ["2", nil]]
2
3
true
true
["a", "b"]
```

---

## `send_literal_and_user` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/send_literal_and_user.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/send_literal_and_user.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:hi` (cluster `P`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/send_literal_and_user.rb:27:in '<main>': undefined method 'hi' for an instance of Mailer (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (4 lines)
10
calc
sent: hi
sent: hi!

--- actual (2 lines)
10
calc

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
test/send_literal_and_user.rb:27:in '<main>': undefined method 'hi' for an instance of Mailer (NoMethodError)
```

---

## `socket_tcp_thread` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/socket_tcp_thread.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/socket_tcp_thread.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-cac9` (cluster `?`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/socket_tcp_thread.rb:14:in '<main>': Unknown error @ read -  (SystemCallError)
```

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (3 lines)
"HTTP/1.0 200 OK\r\n"
true
"GET / HTTP/1.0\r\n"

--- actual (1 lines)
"HTTP/1.0 200 OK\r\n"

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
test/socket_tcp_thread.rb:14:in '<main>': Unknown error @ read -  (SystemCallError)
```

---

## `str_method_nil_arg_no_segv` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/str_method_nil_arg_no_segv.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/str_method_nil_arg_no_segv.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `arity-panic` (cluster `g`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/str_method_nil_arg_no_segv.rb:19:in '<main>': wrong number of arguments (given 0, expected 1+) (ArgumentError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (7 lines)
count: no implicit conversion of nil into String
"foo"
nil
6
send-lshift: can't modify frozen String: "foo"
literal not frozen: b
b

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
test/str_method_nil_arg_no_segv.rb:19:in '<main>': wrong number of arguments (given 0, expected 1+) (ArgumentError)
```

---

## `struct_block_constant_init` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/struct_block_constant_init.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/struct_block_constant_init.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-const:Line` (cluster `P`)

stderr:
```
uninitialized constant Line (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
4
true
false
32

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uninitialized constant Line (NameError)
```

---

## `toplevel_include_module_function` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/toplevel_include_module_function.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/toplevel_include_module_function.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:hello` (cluster `P`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/toplevel_include_module_function.rb:20:in '<main>': undefined method 'hello' for main (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (8 lines)
hello, world!
bye
hi-from-Outer-Inner
shared-from-Second
only-First
only-Second
user-defined
tail-only

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
test/toplevel_include_module_function.rb:20:in '<main>': undefined method 'hello' for main (NoMethodError)
```

---

