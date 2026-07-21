# Conformance failures — full detail

Suite `spinel` — **160 failing test(s)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `3a2e7ef`

Every non-passing test with its source path, the reference it is diffed
against, verdict/bucket, captured stderr, and the full expected-vs-actual
diff — enough to understand each failure without re-running the harness.
Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.

## `analyze_fail/attributes_non_symbol` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/analyze_fail/attributes_non_symbol.rb`
- reference: zeo must reject the program (compile-fail)
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:attributes` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'attributes' for class BadAttributes (NoMethodError)
```

diff:
```
=== stderr diff ===
first difference at line 1
--- expected (2 lines)
/Users/ryanseys/dev/spinel/test/analyze_fail/attributes_non_symbol.rb:2:in '<class:BadAttributes>': undefined method 'attributes' for class BadAttributes (NoMethodError)
	from /Users/ryanseys/dev/spinel/test/analyze_fail/attributes_non_symbol.rb:1:in '<main>'

--- actual (1 lines)
uncaught exception: undefined method 'attributes' for class BadAttributes (NoMethodError)
```

---

## `analyze_fail/instance_exec_def_in_block` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/analyze_fail/instance_exec_def_in_block.rb`
- reference: zeo must reject the program (compile-fail)
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:define_method` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError)
```

diff:
```
=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError)
```

---

## `analyze_fail/instance_exec_define_method` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/analyze_fail/instance_exec_define_method.rb`
- reference: zeo must reject the program (compile-fail)
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:define_method` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError)
```

diff:
```
=== stderr diff ===
first difference at line 1
--- expected (3 lines)
/Users/ryanseys/dev/spinel/test/analyze_fail/instance_exec_define_method.rb:15:in 'block in <main>': undefined method 'define_method' for an instance of BoxPlus (NoMethodError)
	from /Users/ryanseys/dev/spinel/test/analyze_fail/instance_exec_define_method.rb:15:in 'BasicObject#instance_exec'
	from /Users/ryanseys/dev/spinel/test/analyze_fail/instance_exec_define_method.rb:15:in '<main>'

--- actual (1 lines)
uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError)
```

---

## `analyze_fail/instance_exec_no_block` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/analyze_fail/instance_exec_no_block.rb`
- reference: zeo must reject the program (compile-fail)
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-tried-to-create-proc-object-without-block-in-8cab` (cluster `?`)

stderr:
```
uncaught exception: tried to create Proc object without a block (in `instance_exec') (ArgumentError)
```

diff:
```
=== stderr diff ===
first difference at line 1
--- expected (2 lines)
/Users/ryanseys/dev/spinel/test/analyze_fail/instance_exec_no_block.rb:13:in 'BasicObject#instance_exec': no block given (LocalJumpError)
	from /Users/ryanseys/dev/spinel/test/analyze_fail/instance_exec_no_block.rb:13:in '<main>'

--- actual (1 lines)
uncaught exception: tried to create Proc object without a block (in `instance_exec') (ArgumentError)
```

---

## `array_splice_exceptions` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/array_splice_exceptions.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/array_splice_exceptions.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-index-too-small-for-array-minimum-indexerror-4878` (cluster `?`)

stderr:
```
uncaught exception: index -7 too small for array; minimum: -3 (IndexError)
```

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (19 lines)
IndexError: negative length (-2)
IndexError: negative length (-1)
FrozenError
IndexError
IndexError: negative length (-1)
IndexError: index -5 too small for array; minimum: -3
RangeError: -10..1 out of range
RangeError: -10.. out of range
FrozenError
FrozenError
FrozenError
FrozenError
FrozenError
FrozenError
IndexError: negative length (-1)
IndexError: negative length (-1)
RangeError: -9..0 out of range
[9, 3]
[1, 8]

--- actual (5 lines)
IndexError: negative length (-2)
IndexError: negative length (-1)
IndexError
IndexError: negative length (-1)
IndexError: index -5 too small for array; minimum: -3

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: index -7 too small for array; minimum: -3 (IndexError)
```

---

## `bare_return_in_initialize` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bare_return_in_initialize.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bare_return_in_initialize.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 4
--- expected (4 lines)
ok
hi
label
empty

--- actual (4 lines)
ok
hi
label
```

---

## `basicobject_new` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/basicobject_new.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/basicobject_new.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:new` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'new' for class BasicObject (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
:created
Object

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'new' for class BasicObject (NoMethodError)
```

---

## `block_local_fresh_per_iteration` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/block_local_fresh_per_iteration.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/block_local_fresh_per_iteration.rb.expected`
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

## `bundle_array_a` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bundle_array_a.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bundle_array_a.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-index-too-small-for-array-minimum-indexerror-4878` (cluster `?`)

stderr:
```
uncaught exception: index -5 too small for array; minimum: -3 (IndexError)
```

diff:
```
=== stdout diff ===
first difference at line 52
--- expected (117 lines)
5
55
10
1
10
1
1
10
true
false
10
4
8
1
8
8
7
1
2
4
done
[1, 1, 2]
[2, 3]
[2, 2]
[1, 2, 1, 1]
["a", "a", "b"]
["a", "b", "a", "a"]
[1.0, 1.0, 2.0]
[1.5, 1.5]
[1, 1, 2]
["a", "a"]
9
9
1
2
9
9
1
0
0
5
1
3
7
7
7
3
9
9
3
3
9
9
3
5
5
5
1
21
1
8
6
5
3
3
3
3
hello
hello, world, foo
1
1
42
21
3
13
done
1
7
9
1
1
3
0
2
4
1.5
2.5
6
a
b
4
x
y
6
5
0
0
done
5
0
4
8
20
3
42
42
0
3
3
7
11
hello world
foo bar
3
10 20 
30 40 
50 60 

--- actual (51 lines)
5
55
10
1
10
1
1
10
true
false
10
4
8
1
8
8
7
1
2
4
done
[1, 1, 2]
[2, 3]
[2, 2]
[1, 2, 1, 1]
["a", "a", "b"]
["a", "b", "a", "a"]
[1.0, 1.0, 2.0]
[1.5, 1.5]
[1, 1, 2]
["a", "a"]
9
9
1
2
9
9
1
0
0
5
1
3
7
7
7
3
9
9
3
3

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: index -5 too small for array; minimum: -3 (IndexError)
```

---

## `bundle_classd_43` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bundle_classd_43.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bundle_classd_43.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (7 lines)
literal: can't modify frozen String: "abc"
lv-literal: can't modify frozen String: "abc"
Cbc
Zy
Cb
Cbc
ok

--- actual (7 lines)
no raise (literal)
no raise (lv-literal): Cbc
Cbc
Zy
Cb
Cbc
ok
```

---

## `bundle_hash_b2` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bundle_hash_b2.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bundle_hash_b2.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 11
--- expected (29 lines)
5
5
0
false
5
99
10
99
none
none
42
42
1
1
42
42
1
0
2.5
0.0
0.0
1.5
1.5
3.5
1.5
fallback
fallback
-1
got it

--- actual (29 lines)
5
5
0
false
5
99
10
99
none
none


1
1


1
0
2.5
0.0
0.0
1.5
nil
3.5
nil
fallback
fallback
-1
got it
```

---

## `bundle_misc_a` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bundle_misc_a.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bundle_misc_a.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 41
--- expected (208 lines)
42
100
77
done
true
false
false
true
true
true
true
false
true
false
true
false
true
false
true
false
true
true
true
true
true
true
false
false
false
true
false
false
false
false
false
true
true
true
true
true
1
1
0
10
20
30
10
21
hello
world
done
true
true
false
true
caught FiberError
0.5
3.5
1.5
3.5
-3.25
2.75
4.5
4.5
4.5
3
2.5
3.5
4.5
2
2.5
3.5
2
4.5
5.5
3
3.5
5.5
1.5
5.5
10.5
1.5
3
2.5
2.5
3.5
4.5
0
3.14
3.1416
1.5
2.5
3.15
3.1416
1.01
3.14
3.1415
1.99
3.14
3.1415
-1.56
3
4
3
3
true
true
true
true
true
true
1.0
100.0
-3.25
1234567890.0
1234567890.5
0.1
0.3
0.30000000000000004
0.0001
1.0e-05
150000000000000.0
1.0e+15
9.99e+15
1.0e+16
1.0e+100
-0.0
Infinity
-Infinity
NaN
1.0
1234567890.5
1.0e+16
-0.0
3.14
0.0
-2.5
1.0
42.0
2.71
100.0
0
1000
0
479
877
546
785
785
1570
1570
1414
0
3000
2000
1000
1584
477
5000
462
761
1175
1543
549
881
1316
3141
5:03
hello world 42
42
green
yellow
1
2
5
3
4
3
3
4
false
true
true
false
true
3.14
1024
5
HELLO
2
5
done
5
1,2,3,4,5
1,2,3,4,5
4
true
true
true
true
foo,bar,baz,qux
3
10,20,30
4
4
4
0
42

--- actual (208 lines)
42
100
77
done
true
false
false
true
true
true
true
false
true
false
true
false
true
false
true
false
true
true
true
true
true
true
false
false
false
true
false
false
false
false
false
true
true
true
true
true
0
0
0
10
20
30
10
21
hello
world
done
true
true
false
true
caught FiberError
0.5
3.5
1.5
3.5
-3.25
2.75
4.5
4.5
4.5
3
2.5
3.5
4.5
2
2.5
3.5
2
4.5
5.5
3
3.5
5.5
1.5
5.5
10.5
1.5
3
2.5
2.5
3.5
4.5
0
3.14
3.1416
1.5
2.5
3.15
3.1416
1.01
3.14
3.1415
1.99
3.14
3.1415
-1.56
3
4
3
3
true
true
true
true
true
true
1.0
100.0
-3.25
1234567890.0
1234567890.5
0.1
0.3
0.30000000000000004
0.0001
1.0e-05
150000000000000.0
1.0e+15
9.99e+15
1.0e+16
1.0e+100
-0.0
Infinity
-Infinity
NaN
1.0
1234567890.5
1.0e+16
-0.0
3.14
0.0
-2.5
1.0
42.0
2.71
100.0
0
1000
0
479
877
546
785
785
1570
1570
1414
0
3000
2000
1000
1584
477
5000
462
761
1175
1543
549
881
1316
3141
5:03
hello world 42
42
green
yellow
1
2
5
3
4
3
3
4
false
true
true
false
true
3.14
1024
5
HELLO
2
5
done
5
1,2,3,4,5
1,2,3,4,5
4
true
true
true
true
foo,bar,baz,qux
3
10,20,30
4
4
4
0
42
```

---

## `bundle_misc_c_08` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bundle_misc_c_08.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bundle_misc_c_08.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 27
--- expected (33 lines)
true
true
true
true
[3, 5, 7]
[10, 10, 10]
[[1, 2], [2, 3]]
[]
[31, 52, 73]
[11, 12, 13]
[1, 2]
[[1, 1], [1, 2], [1, 3]]
fallback-value
computed-default
pre-fix
set=ok
unset=fallback-used
true
true
true
true
true
true
true
true
true
true
true
true
42
42
7
7

--- actual (33 lines)
true
true
true
true
[3, 5, 7]
[10, 10, 10]
[[1, 2], [2, 3]]
[]
[31, 52, 73]
[11, 12, 13]
[1, 2]
[[1, 1], [1, 2], [1, 3]]
fallback-value
computed-default
pre-fix
set=ok
unset=fallback-used
true
true
true
true
true
true
true
true
true
false
true
true
42
42
7
7
```

---

## `bundle_misc_c_09` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bundle_misc_c_09.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bundle_misc_c_09.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:[]=` (cluster `P`)

stderr:
```
uncaught exception: undefined method '[]=' for an instance of Fiber (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 4
--- expected (6 lines)
42
43
nil_ok
100
200
survived

--- actual (3 lines)
42
43
nil_ok

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method '[]=' for an instance of Fiber (NoMethodError)
```

---

## `bundle_misc_c_36` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bundle_misc_c_36.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bundle_misc_c_36.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:length` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'length' for nil (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 12
--- expected (59 lines)
5
0.75
1.25
2.0
3.5
4.125
3.5
1.25
4294967296
16777621
3601407474
true
true
11
10
11
"hi"
"lo"
"hi"
"lo"
true
false
true
true
false
true
false
true
false
false
found-0
missing
found-42
found-str-0
missing-str-key
1, 2, 3, 4, 5
5
true
0
done
4
1
0
0
0
0
0
0
1
0
0
0
0
0
0
0
1
0
5

--- actual (11 lines)
5
0.75
1.25
2.0
3.5
4.125
3.5
1.25
4294967296
16777621
3601407474

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'length' for nil (NoMethodError)
```

---

## `bundle_tiny_string` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/bundle_tiny_string.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/bundle_tiny_string.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `arity-panic` (cluster `g`)

stderr:
```
uncaught exception: wrong number of arguments (given 2, expected 0..1) (ArgumentError)
```

diff:
```
=== stdout diff ===
first difference at line 18
--- expected (78 lines)
true
true
true
true
"a"
"b"
"c"
---
"a\n"
"b\n"
"c"
---
"x"
"y"
["a", "b", "c"]
["a\n", "b\n", "c"]
["a", "b", "c"]
"bc"
"def"
"ccc"
true
false
true
true
false
"hello"
"hello"
"hello"
"hello"
"hello"
[104, 101, 108, 108, 111]
[12354]
[]
"hello"
"heo"
"hello"
"ho"
"heo"
"ll"
xhxexlxlxox
-a-b-c-
["a", "b", "c", "d", "e"]
["a", "b", "c"]
["x", "y", "z", "!"]
[" ", "!", "\"", "#", "$", "%", "&"]
[["hello"], ["world"]]
[["a", "1"], ["b", "22"]]
["hello", "world"]
["ab", "ac"]
-1
1
0
-1
"aaabbbccc"
"abbbccc"
"abc"
"abc"
"abc"
"abc"
"abc"
"x"
""
"no change"
hello
:foo
hero
xbbbccc
herro
xy
true
true
true
false
false
11
11
5
3

--- actual (53 lines)
true
true
true
true
"a"
"b"
"c"
---
"a\n"
"b\n"
"c"
---
"x"
"y"
["a", "b", "c"]
["a\n", "b\n", "c"]
["a", "b", "c"]
"abc"
"abcdef"
"aabbbccc"
true
false
true
true
false
"hello"
"hello"
"hello"
"hello"
"hello"
[104, 101, 108, 108, 111]
[12354]
[]
"hello"
"heo"
"hello"
"ho"
"heo"
"ll"
xhxexlxlxox
-a-b-c-
["a", "b", "c", "d", "e"]
["a", "b", "c"]
["x", "y", "z", "!"]
[" ", "!", "\"", "#", "$", "%", "&"]
[["hello"], ["world"]]
[["a", "1"], ["b", "22"]]
["hello", "world"]
["ab", "ac"]
-1
1
0
-1

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: wrong number of arguments (given 2, expected 0..1) (ArgumentError)
```

---

## `case_when_lambda_predicate` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/case_when_lambda_predicate.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/case_when_lambda_predicate.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (8 lines)
story
comment
other
big
even
other
big7
no2

--- actual (8 lines)
other
other
other
other
other
other
no
no2
```

---

## `case_when_string_range_nil_bound` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/case_when_string_range_nil_bound.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/case_when_string_range_nil_bound.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
"in"
"out"
"in"
"out"
"in"
"in"

--- actual (6 lines)
"out"
"out"
"out"
"out"
"in"
"out"
```

---

## `chunk_family_enumerator_value` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/chunk_family_enumerator_value.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/chunk_family_enumerator_value.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (8 lines)
#<Enumerator: #<Enumerator::Generator:0xADDR>:each>
[[1, 2], [4, 5]]
#<Enumerator: #<Enumerator::Generator:0xADDR>:each>
[[1, 2], [4, 5]]
#<Enumerator: #<Enumerator::Generator:0xADDR>:each>
[[3, 4], [7]]
[["aa", "ab"], ["ba"]]
[[1, 2], [4, 5]]

--- actual (8 lines)
[[1, 2], [4, 5]]
[[1, 2], [4, 5]]
[[1, 2], [4, 5]]
[[1, 2], [4, 5]]
[[3, 4], [7]]
[[3, 4], [7]]
[["aa", "ab"], ["ba"]]
[[1, 2], [4, 5]]
```

---

## `compile_time_attribute_singular` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/compile_time_attribute_singular.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/compile_time_attribute_singular.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:attribute` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'attribute' for class CompileTimeSingleAttribute (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
Ada
Grace

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'attribute' for class CompileTimeSingleAttribute (NoMethodError)
```

---

## `compile_time_attribute_wrapped_record` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/compile_time_attribute_wrapped_record.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/compile_time_attribute_wrapped_record.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:attributes` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'attributes' for class WrappedResult (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
true
manual:reviewed

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'attributes' for class WrappedResult (NoMethodError)
```

---

## `compile_time_attributes` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/compile_time_attributes.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/compile_time_attributes.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:attributes` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
Ada
2
5

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder (NoMethodError)
```

---

## `compile_time_define_method_predicates` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/compile_time_define_method_predicates.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/compile_time_define_method_predicates.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:attributes` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'attributes' for class CompileTimePostState (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
true
false
false
true

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'attributes' for class CompileTimePostState (NoMethodError)
```

---

## `const_aliased_class_reopen_include` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/const_aliased_class_reopen_include.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/const_aliased_class_reopen_include.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:wordy` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'wordy' for an instance of Integer (NoMethodError)
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
uncaught exception: undefined method 'wordy' for an instance of Integer (NoMethodError)
```

---

## `constant_path` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/constant_path.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/constant_path.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-const:M::C` (cluster `P`)

stderr:
```
uncaught exception: uninitialized constant M::C (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 1
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

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: uninitialized constant M::C (NameError)
```

---

## `data_dup_clone_frozen` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/data_dup_clone_frozen.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/data_dup_clone_frozen.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
true
true
true
1
false
false

--- actual (6 lines)
false
false
true
1
false
false
```

---

## `data_hash_key` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/data_hash_key.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/data_hash_key.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
"a"
"a"

--- actual (2 lines)
nil
nil
```

---

## `data_new_validation` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/data_new_validation.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/data_new_validation.rb.expected`
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

## `data_value_eq_container` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/data_value_eq_container.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/data_value_eq_container.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (9 lines)
true
true
1
1
true
true
true
true
false

--- actual (9 lines)
false
false
nil
2
false
false
false
true
false
```

---

## `empty_array_push_ptr` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/empty_array_push_ptr.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/empty_array_push_ptr.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
2
p1=match
p2=match

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

---

## `enum_terminal_chunk_zip_lazy` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/enum_terminal_chunk_zip_lazy.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/enum_terminal_chunk_zip_lazy.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-no-block-given-yield-localjumperror-1be0` (cluster `?`)

stderr:
```
uncaught exception: no block given (yield) (LocalJumpError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (10 lines)
[104, 105]
2
{1 => 2, 3 => 4}
2
[1, 2, 3]
[[false, [1]], [true, [2]], [false, [3]], [true, [4]]]
[[1, 4], [2, 5], [3, 6]]
[[1, 3, 5], [2, 4, 6]]
[1, 4, 9]
[1, 2]

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: no block given (yield) (LocalJumpError)
```

---

## `exception_value_parity` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/exception_value_parity.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/exception_value_parity.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (18 lines)
true
false
false
true
false
key not found: :missing_key
key not found: :b
5
Math::DomainError
Numerical argument is out of domain - sqrt
NameError
undefined method 'absent' for class 'Vis'
false
:foo
NoMethodError
#<ArgumentError: root>
top
#<ArgumentError: inner>

--- actual (18 lines)
NoMethodError
undefined method 'private' for class Vis
true
false
false
true
false
key not found: :missing_key
key not found: :b
5
Math::DomainError
Numerical argument is out of domain - sqrt
false
:foo
NoMethodError
#<ArgumentError: root>
top
#<ArgumentError: inner>
```

---

## `ffi_array_specs_decl` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_array_specs_decl.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_array_specs_decl.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module BulkF (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
ok

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module BulkF (NoMethodError)
```

---

## `ffi_binstr_recv` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_binstr_recv.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_binstr_recv.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module Net (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
5
5
[97, 0, 98, 0, 99]

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module Net (NoMethodError)
```

---

## `ffi_binstr_ws_frame` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_binstr_ws_frame.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_binstr_ws_frame.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module Bin (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
frame bytes (binstr)  = 135
truncated (str)       = 2
unmasked length       = 127
unmasked == subscribe = true

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module Bin (NoMethodError)
```

---

## `ffi_buffer_reader` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_buffer_reader.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_buffer_reader.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_buffer` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_buffer' for module Buf (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
0
0
0
0
0

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_buffer' for module Buf (NoMethodError)
```

---

## `ffi_callback` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_callback.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_callback.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_callback` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_callback' for module L (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
[1, 1, 2, 3, 4, 5, 6, 9]
[40, 30, 20, 10]
5
miss
done
at exit

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_callback' for module L (NoMethodError)
```

---

## `ffi_cflags_fold_forms` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_cflags_fold_forms.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_cflags_fold_forms.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_lib` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_lib' for module MathA (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
1.5
1.0

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_lib' for module MathA (NoMethodError)
```

---

## `ffi_const` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_const.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_const.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_const` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_const' for module Flags (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
1
2
4
255
7

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_const' for module Flags (NoMethodError)
```

---

## `ffi_const_leaf_collision` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_const_leaf_collision.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_const_leaf_collision.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_lib` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_lib' for module CMath (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
int
download-text
4
2.5

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_lib' for module CMath (NoMethodError)
```

---

## `ffi_const_nested_module_path` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_const_nested_module_path.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_const_nested_module_path.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_lib` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_lib' for module Outer::CMath (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
int
verbose
8
1.5

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_lib' for module Outer::CMath (NoMethodError)
```

---

## `ffi_foreign_ptr_gc` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_foreign_ptr_gc.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_foreign_ptr_gc.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_lib` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_lib' for module F (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
ok 200

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_lib' for module F (NoMethodError)
```

---

## `ffi_int_arg_bigint` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_int_arg_bigint.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_int_arg_bigint.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_lib` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_lib' for module M (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
40

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_lib' for module M (NoMethodError)
```

---

## `ffi_int_arg_poly_value` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_int_arg_poly_value.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_int_arg_poly_value.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module Lib (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
42

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module Lib (NoMethodError)
```

---

## `ffi_libc_libm_basic` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_libc_libm_basic.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_libc_libm_basic.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module LibM (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
1
4
1024
12
pid_ok

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module LibM (NoMethodError)
```

---

## `ffi_poly_int_array` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_poly_int_array.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_poly_int_array.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module L (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
"A"
TypeError: no implicit conversion into an FFI :int_array

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module L (NoMethodError)
```

---

## `ffi_ptr_array` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_ptr_array.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_ptr_array.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
2

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

---

## `ffi_ptr_int_literal` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_ptr_int_literal.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_ptr_int_literal.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
ok

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

---

## `ffi_ptr_nil` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_ptr_nil.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_ptr_nil.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
non_nil
zero_is_nil

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

---

## `ffi_str_arg_poly_value` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_str_arg_poly_value.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_str_arg_poly_value.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module Lib (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
42

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module Lib (NoMethodError)
```

---

## `ffi_struct` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_struct.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_struct.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_struct` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_struct' for module M (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (11 lines)
3
7
10
42
3.5
11
true
true
true
hello
5

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_struct' for module M (NoMethodError)
```

---

## `ffi_variadic` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_variadic.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_variadic.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module C (NoMethodError)
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

## `ffi_void_return` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_void_return.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_void_return.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
freed
42

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError)
```

---

## `ffi_write_roundtrip` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/ffi_write_roundtrip.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/ffi_write_roundtrip.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_buffer` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_buffer' for module Buf (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
-12345
4000000000
77
77
true

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_buffer' for module Buf (NoMethodError)
```

---

## `float_pow_negative_fractional` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/float_pow_negative_fractional.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/float_pow_negative_fractional.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
Math::DomainError: raised
1.4142135623730951
4.0
raised2

--- actual (4 lines)
NaN
1.4142135623730951
4.0
NaN
```

---

## `frozen_string_literal_per_file` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/frozen_string_literal_per_file.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/frozen_string_literal_per_file.rb.expected`
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

- source: `/Users/ryanseys/dev/spinel/test/frozen_string_literal_per_file_rev.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/frozen_string_literal_per_file_rev.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-can-modify-frozen-string-plain-frozenerror-b7c8` (cluster `?`)

stderr:
```
uncaught exception: can't modify frozen String: "plain" (FrozenError)
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

--- actual (1 lines)
uncaught exception: can't modify frozen String: "plain" (FrozenError)
```

---

## `gc_stat_string_heap` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/gc_stat_string_heap.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/gc_stat_string_heap.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:>=` (cluster `P`)

stderr:
```
uncaught exception: undefined method '>=' for nil (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
str_count >= 1000: yes
str_bytes > 0: yes
retained: 1000

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method '>=' for nil (NoMethodError)
```

---

## `harness_batch_2453_2456` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/harness_batch_2453_2456.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/harness_batch_2453_2456.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-const:OpenSSL` (cluster `P`)

stderr:
```
uncaught exception: uninitialized constant OpenSSL (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (4 lines)
0.903
4
true
raised

--- actual (1 lines)
0.903

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: uninitialized constant OpenSSL (NameError)
```

---

## `hash_dig` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/hash_dig.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/hash_dig.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-integer-does-not-have-dig-method-typeerror-ee2a` (cluster `?`)

stderr:
```
uncaught exception: Integer does not have #dig method (TypeError)
```

diff:
```
=== stdout diff ===
first difference at line 31
--- expected (37 lines)
1
2
alpha
beta
x
y
10
20
Alice
30
7
100
50
2
hi
sayonara
42


Bob
25

0
nil
""
nil
1
nil
hi
nil
nil
k1
k2
99
nil
nil
nil

--- actual (30 lines)
1
2
alpha
beta
x
y
10
20
Alice
30
7
100
50
2
hi
sayonara
42


Bob
25

0
nil
""
nil
1
nil
hi
nil

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: Integer does not have #dig method (TypeError)
```

---

## `hash_each_with_object` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/hash_each_with_object.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/hash_each_with_object.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:<<` (cluster `P`)

stderr:
```
uncaught exception: undefined method '<<' for nil (NoMethodError)
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

--- actual (1 lines)
uncaught exception: undefined method '<<' for nil (NoMethodError)
```

---

## `hash_flatten_depth` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/hash_flatten_depth.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/hash_flatten_depth.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (7 lines)
[:a, [1, 2], :b, 3]
[[:a, [1, 2]], [:b, 3]]
[:a, [1, 2], :b, 3]
[:a, 1, 2, :b, 3]
[:a, 1, 2, :b, 3]
[:x, 1, [2, 3], :y, 4]
[:x, 1, 2, 3, :y, 4]

--- actual (7 lines)
[:a, [1, 2], :b, 3]
[:a, [1, 2], :b, 3]
[:a, [1, 2], :b, 3]
[:a, 1, 2, :b, 3]
[:a, [1, 2], :b, 3]
[:x, 1, [2, 3], :y, 4]
[:x, 1, 2, 3, :y, 4]
```

---

## `hash_numeric_wave11` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/hash_numeric_wave11.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/hash_numeric_wave11.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 17
--- expected (26 lines)
true
false
true
false
1
true
[1, 2]
Infinity
"TypeError"
"TypeError"
2
2
2
2.0
-2.0
2.0
[3, (1/1)]
(1/1)
(1/1)
0.0
3.0
2.0
1.0
1
3
2.5

--- actual (26 lines)
true
false
true
false
1
true
[1, 2]
Infinity
"TypeError"
"TypeError"
2
2
2
2.0
-2.0
2.0
[(10/3), (1/1)]
(1/1)
(1/1)
0.0
3.0
2.0
1.0
1
3
2.5
```

---

## `hash_variant_returns` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/hash_variant_returns.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/hash_variant_returns.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 10
--- expected (11 lines)
{a: 1, c: 3}
{}
[:a, 1, :b, 2]
[[0, :a], [1, :b]]
{1 => 1, 2 => 4}
{a: 1, b: 2}
[[false, [[:a, 1]]], [true, [[:b, 2], [:c, 4]]]]
[[true, [[:a, 2], [:b, 4], [:c, 6]]]]
[["a", [["x", "apple"], ["y", "avocado"]]], ["b", [["z", "berry"]]]]
[[:pos, [[:a, 1]]], [:pos, [[:c, 3]]]]
[[:_alone, [[:a, 1]]], [:_alone, [[:b, 1]]]]

--- actual (11 lines)
{a: 1, c: 3}
{}
[:a, 1, :b, 2]
[[0, :a], [1, :b]]
{1 => 1, 2 => 4}
{a: 1, b: 2}
[[false, [[:a, 1]]], [true, [[:b, 2], [:c, 4]]]]
[[true, [[:a, 2], [:b, 4], [:c, 6]]]]
[["a", [["x", "apple"], ["y", "avocado"]]], ["b", [["z", "berry"]]]]
[[:pos, [[:a, 1]]], [nil, [[:b, 0]]], [:pos, [[:c, 3]]]]
[[:_alone, [[:a, 1], [:b, 1]]]]
```

---

## `i1011` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/i1011.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/i1011.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_cflags` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_cflags' for module Pathy (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
5
0

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_cflags' for module Pathy (NoMethodError)
```

---

## `i1017` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/i1017.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/i1017.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_buffer` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_buffer' for module Buf (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
ok

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_buffer' for module Buf (NoMethodError)
```

---

## `i1021` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/i1021.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/i1021.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:>=` (cluster `P`)

stderr:
```
uncaught exception: undefined method '>=' for nil (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (9 lines)
true
true
true
true
true
true
true
true
done

--- actual (5 lines)
false
false
false
false
false

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method '>=' for nil (NoMethodError)
```

---

## `i918` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/i918.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/i918.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (13 lines)
true
array << raised
array []= raised
array push raised
[1, 2, 3]
false
[9, 2, 3, 4]
str_array << raised
["x", "y"]
true
hash []= raised
false
2

--- actual (10 lines)
true
array []= raised
[1, 2, 3, 4, 5]
false
[9, 2, 3, 4]
["x", "y", "z"]
true
hash []= raised
false
2
```

---

## `include_chain_module_nested_const` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/include_chain_module_nested_const.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/include_chain_module_nested_const.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-const:RequestDispatch::ViewHelpers` (cluster `P`)

stderr:
```
uncaught exception: uninitialized constant RequestDispatch::ViewHelpers (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
42
deep

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: uninitialized constant RequestDispatch::ViewHelpers (NameError)
```

---

## `inline_yield_method_with_return` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/inline_yield_method_with_return.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/inline_yield_method_with_return.rb.expected`
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

## `inspect` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/inspect.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/inspect.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 46
--- expected (52 lines)
5
-42
0
1.5
1.0
-3.25
"hi"
""
"a\nb"
"tab\there"
"quote\"inside"
"back\\slash"
:foo
true
false
nil
got 42
5
1.0
"hi"
:foo
true
nil
[]
[1, 2, 3]
[-5, 0, 42]
[99]
[1.5, 2.0]
[1.0]
["hello", "world"]
[""]
[:foo, :bar]
[1, "x"]
[nil, :sym, 1.5]
[1, "two", :three, 4.0, true]
[false, nil]
[1, "x"]
[1, "x"]
[1, 2, 3]
[1, 2, 3]
got [1, 2, 3]
[10, 20, 30]
1
2
3
a
b
x
y
1.5
2.0
done

--- actual (53 lines)
5
-42
0
1.5
1.0
-3.25
"hi"
""
"a\nb"
"tab\there"
"quote\"inside"
"back\\slash"
:foo
true
false
nil
got 42
5
1.0
"hi"
:foo
true
nil
[]
[1, 2, 3]
[-5, 0, 42]
[99]
[1.5, 2.0]
[1.0]
["hello", "world"]
[""]
[:foo, :bar]
[1, "x"]
[nil, :sym, 1.5]
[1, "two", :three, 4.0, true]
[false, nil]
[1, "x"]
[1, "x"]
[1, 2, 3]
[1, 2, 3]
got [1, 2, 3]
[10, 20, 30]
1
2
3

a
b
x
y
1.5
2.0
done
```

---

## `integer_argument_error` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/integer_argument_error.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/integer_argument_error.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 19
--- expected (36 lines)
42
0
-7
5
-7
42
42
42
42
42
-7
0
0
-1
-1
-1
-1
9223372036854775807
-1
-1
-1
-1
-1
-1
-1
-1
-1
42
-1
-1
msg: invalid value for Integer(): "nope"
stress ok: 100
42
0
0
-7

--- actual (36 lines)
42
0
-7
5
-7
42
42
42
42
42
-7
0
0
-1
-1
-1
-1
9223372036854775807
-9223372036854775808
-1
-1
-1
-1
-1
-1
-1
-1
42
-1
-1
msg: invalid value for Integer(): "nope"
stress ok: 100
42
0
0
-7
```

---

## `integer_chr_encoding` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/integer_chr_encoding.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/integer_chr_encoding.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 14
--- expected (15 lines)
"A"
"あ"
[194, 128]
[223, 191]
[239, 191, 191]
[244, 143, 191, 191]
"A"
"A"
[200]
-1.chr: RangeError: -1 out of char range
256.chr: RangeError: 256 out of char range
12354.chr: RangeError: 12354 out of char range
-1.chr(UTF_8): RangeError: -1 out of char range
55296.chr(UTF_8): RangeError: invalid codepoint 0xD800 in UTF-8
1114112.chr(UTF_8): RangeError: 1114112 out of char range

--- actual (15 lines)
"A"
"あ"
[194, 128]
[223, 191]
[239, 191, 191]
[244, 143, 191, 191]
"A"
"A"
[200]
-1.chr: RangeError: -1 out of char range
256.chr: RangeError: 256 out of char range
12354.chr: RangeError: 12354 out of char range
-1.chr(UTF_8): RangeError: -1 out of char range
55296.chr(UTF_8): RangeError: 55296 out of char range
1114112.chr(UTF_8): RangeError: 1114112 out of char range
```

---

## `issue_2962` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_2962.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_2962.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
(-5+12i)
(1.6741492280355401+0.895977476129838i)
(-46+9i)

--- actual (3 lines)
(-5.0+12.0i)
(1.6741492280355401+0.895977476129838i)
(-46.00000000000001+9.000000000000009i)
```

---

## `issue_2970` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_2970.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_2970.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
true

--- actual (1 lines)
false
```

---

## `issue_2975` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_2975.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_2975.rb.expected`
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
4124922702142622782
S
P
false
```

---

## `issue_2993` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_2993.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_2993.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:with_index` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'with_index' for an instance of Enumerator::Lazy (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (6 lines)
[1, 2, 3]
[[1, 2], [4, 5], [7]]
[[10, 0], [20, 1], [30, 2]]
[1, 2]
[2, 4, 6]
[5, 10]

--- actual (2 lines)
[1, 2, 3]
[[1, 2], [4, 5], [7]]

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'with_index' for an instance of Enumerator::Lazy (NoMethodError)
```

---

## `issue_3000` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3000.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3000.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (8 lines)
TypeError
TypeError
TypeError
TypeError
["ZZ_A", "zzv"]
"ZZ_A"
{"ZZ_A" => "zzv"}
["zzv"]

--- actual (8 lines)
nil
TypeError
{}
[nil]
["ZZ_A", "zzv"]
"ZZ_A"
{"ZZ_A" => "zzv"}
["zzv"]
```

---

## `issue_3002` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3002.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3002.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
true
[1]
["x"]
FrozenError

--- actual (4 lines)
[1, 2, 3]
[1, 2]
["x", "y"]
FrozenError
```

---

## `issue_3024` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3024.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3024.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
5
[:break, nil]
"str"

--- actual (3 lines)
nil
[:break, nil]
nil
```

---

## `issue_3024b` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3024b.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3024b.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
7
10

--- actual (2 lines)
nil
nil
```

---

## `issue_3037` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3037.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3037.rb.expected`
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

## `issue_3048` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3048.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3048.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (7 lines)
false
42
false
2
2
true
42

--- actual (7 lines)
true
42
true
2
2
true
42
```

---

## `issue_3049` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3049.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3049.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
Infinity
NaN
Float
true
true

--- actual (5 lines)
no-raise-inf
no-raise-nan
Float
true
true
```

---

## `issue_3049b` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3049b.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3049b.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
Errno::EDOM
Errno::EDOM
Errno::EDOM
Errno::EDOM
Float
Errno::EDOM

--- actual (6 lines)
Infinity
ArgumentError
NaN
Infinity
ArgumentError
Infinity
```

---

## `issue_3051` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3051.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3051.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (8 lines)
false
-1
8
true
true
false
false
true

--- actual (8 lines)
true
-1
8
true
true
true
true
true
```

---

## `issue_3057` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3057.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3057.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `arity-panic` (cluster `g`)

stderr:
```
uncaught exception: wrong number of arguments (given 1, expected 0) (ArgumentError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
(1/3)
(1/2)
(22/7)
(2/3)
(1/3)

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: wrong number of arguments (given 1, expected 0) (ArgumentError)
```

---

## `issue_3061` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3061.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3061.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (7 lines)
a/b
/a\/b/
(?-mix:a\/b)
/foo\/bar/
/a\/b\/c/mi
(?mi-x:a\/b\/c)
/x/

--- actual (7 lines)
a/b
/a/b/
(?-mix:a/b)
/foo/bar/
/a/b/c/mi
(?mi-x:a/b/c)
/x/
```

---

## `issue_3069` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3069.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3069.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
true
false
true
true

--- actual (4 lines)
false
false
true
false
```

---

## `issue_3088` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3088.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3088.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
ArgumentError
ArgumentError
UncaughtThrowError

--- actual (3 lines)
#<UncaughtThrowError: UncaughtThrowError>
#<UncaughtThrowError: tag>
UncaughtThrowError
```

---

## `issue_3098` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3098.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3098.rb.expected`
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

## `issue_3101` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3101.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3101.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
true
false
true
false
true
true

--- actual (6 lines)
false
false
false
false
false
false
```

---

## `issue_3119` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3119.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3119.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-no-receiver-is-available-argumenterror-234c` (cluster `?`)

stderr:
```
uncaught exception: no receiver is available (ArgumentError)
```

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (3 lines)
{a: 1}
true
true

--- actual (2 lines)
{a: 1}
true

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: no receiver is available (ArgumentError)
```

---

## `issue_3126` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3126.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3126.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
true
true
true
true

--- actual (4 lines)
false
false
true
true
```

---

## `issue_3135` — FAIL_COMPILE

- source: `/Users/ryanseys/dev/spinel/test/issue_3135.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3135.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_COMPILE (stage `compile`) · bucket `auto-cannot-load-such-file-ostruct-0485` (cluster `?`)

stderr:
```
zeo: cannot load such file -- ostruct
```

---

## `issue_3155` — FAIL_COMPILE

- source: `/Users/ryanseys/dev/spinel/test/issue_3155.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3155.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_COMPILE (stage `compile`) · bucket `auto-cannot-load-such-file-ostruct-0485` (cluster `?`)

stderr:
```
zeo: cannot load such file -- ostruct
```

---

## `issue_3163` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3163.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3163.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
true
true
true
false
false
false

--- actual (6 lines)
false
false
false
true
false
false
```

---

## `issue_3179` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3179.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3179.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-const:A` (cluster `P`)

stderr:
```
uncaught exception: uninitialized constant A (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
"a:5"
"b:9"
"A 1"
"C 2 3"
"none"

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: uninitialized constant A (NameError)
```

---

## `issue_3180` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/issue_3180.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3180.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-const:User` (cluster `P`)

stderr:
```
uncaught exception: uninitialized constant User (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
"Alice"
"Alice"

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: uninitialized constant User (NameError)
```

---

## `issue_3193` — FAIL_COMPILE

- source: `/Users/ryanseys/dev/spinel/test/issue_3193.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3193.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_COMPILE (stage `compile`) · bucket `auto-cannot-load-such-file-ostruct-0485` (cluster `?`)

stderr:
```
zeo: cannot load such file -- ostruct
```

---

## `issue_3194` — FAIL_COMPILE

- source: `/Users/ryanseys/dev/spinel/test/issue_3194.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3194.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_COMPILE (stage `compile`) · bucket `auto-cannot-load-such-file-ostruct-0485` (cluster `?`)

stderr:
```
zeo: cannot load such file -- ostruct
```

---

## `issue_3197` — FAIL_COMPILE

- source: `/Users/ryanseys/dev/spinel/test/issue_3197.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/issue_3197.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_COMPILE (stage `compile`) · bucket `auto-cannot-load-such-file-ostruct-0485` (cluster `?`)

stderr:
```
zeo: cannot load such file -- ostruct
```

---

## `kernel_array_format_negx_wave10` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/kernel_array_format_negx_wave10.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/kernel_array_format_negx_wave10.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (17 lines)
(2+3i)
(1/6)
[10, (1/6)]
10
10.5
(21/2)
(1/6)
[1, 2, 3]
[[:a, 1]]
[]
"..f"
"..70"
"ff"
"-1"
"..F"
"..f00"
"100"

--- actual (17 lines)
(2+3i)
(1/6)
[(21/2), (1/6)]
10
10.5
(21/2)
0.16666666666666696
[1, 2, 3]
[[:a, 1]]
[]
"..f"
"..70"
"ff"
"-1"
"..F"
"..f00"
"100"
```

---

## `kernel_float_integer_strict` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/kernel_float_integer_strict.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/kernel_float_integer_strict.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 6
--- expected (24 lines)
F "1_000.5" 1000.5
F "1_0.5_5" 10.55
F "1e1_0" 10000000000.0
F "0x1_1" 17.0
F "0x1_1.0" 17.0
F "1__0" "AE"
F "1_" "AE"
F "_1" "AE"
F "1_e3" "AE"
F "1e_3" "AE"
F ".5" 0.5
F "5." 5.0
F "inf" "AE"
F "Infinity" "AE"
F "nan" "AE"
F " +3.25 " 3.25
F NUL "AE"
I "1_000" 1000
I "077" 63
I "0x1A" 26
I "0b101" 5
I " -7 " -7
I "1__0" "AE"
I NUL "AE"

--- actual (24 lines)
F "1_000.5" 1000.5
F "1_0.5_5" 10.55
F "1e1_0" 10000000000.0
F "0x1_1" 17.0
F "0x1_1.0" 17.0
F "1__0" 10.0
F "1_" 1.0
F "_1" 1.0
F "1_e3" 1000.0
F "1e_3" 1000.0
F ".5" 0.5
F "5." 5.0
F "inf" Infinity
F "Infinity" Infinity
F "nan" "AE"
F " +3.25 " 3.25
F NUL "AE"
I "1_000" 1000
I "077" 63
I "0x1A" 26
I "0b101" 5
I " -7 " -7
I "1__0" "AE"
I NUL "AE"
```

---

## `module_class_body_side_effects` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/module_class_body_side_effects.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/module_class_body_side_effects.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (7 lines)
before module
in module body
version is 1.0
after module
in class body
deep body ran
hi

--- actual (7 lines)
in module body
version is 1.0
in class body
deep body ran
before module
after module
hi
```

---

## `native_binding_poc` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/native_binding_poc.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/native_binding_poc.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:native_obj` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'native_obj' for module NB (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
{"a":1,"b":[2,3]}!
42
["x",null,true]
9

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'native_obj' for module NB (NoMethodError)
```

---

## `numeric_edges_wave10` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/numeric_edges_wave10.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/numeric_edges_wave10.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-hash-can-be-coerced-into-integer-typeerror-8a63` (cluster `?`)

stderr:
```
uncaught exception: Hash can't be coerced into Integer (TypeError)
```

diff:
```
=== stdout diff ===
first difference at line 8
--- expected (20 lines)
2
5
"TypeError"
[4, 0.0]
[3, 1]
3.5
2.5
10
11
"FDE"
"FDE2"
[0, 7.0]
1
[-1, -Infinity]
20
20
20
30
"Infinity"
Infinity

--- actual (7 lines)
2
5
"TypeError"
[4, 0.0]
[3, 1]
3.5
2.5

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: Hash can't be coerced into Integer (TypeError)
```

---

## `object_freeze_module_isa` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/object_freeze_module_isa.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/object_freeze_module_isa.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 2
--- expected (10 lines)
false
true
true
false
true
true
true
true
false
true

--- actual (10 lines)
false
false
true
false
false
true
true
true
false
true
```

---

## `object_identity_cluster` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/object_identity_cluster.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/object_identity_cluster.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 7
--- expected (16 lines)
true
false
false
true
false
true
true
true
true
true
true
true
[Enumerable]
[Comparable]
true
NoMethodError

--- actual (16 lines)
true
false
false
true
false
true
false
true
true
true
true
true
[Enumerable]
[Comparable]
true
nil
```

---

## `op_assign_tail_return` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/op_assign_tail_return.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/op_assign_tail_return.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (11 lines)
11
8
12
11
12
17
17
11
12
105
-105

--- actual (11 lines)



11
12
17
17
11
12
```

---

## `param_body_hash_inference` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/param_body_hash_inference.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/param_body_hash_inference.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:[]` (cluster `P`)

stderr:
```
uncaught exception: undefined method '[]' for nil (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
id=
title=Real Hash
title=
title=Another Real
name=Sym Hash
name=

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method '[]' for nil (NoMethodError)
```

---

## `param_include_body_widen` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/param_include_body_widen.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/param_include_body_widen.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:include?` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'include?' for nil (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
false
true
false
true
false

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'include?' for nil (NoMethodError)
```

---

## `param_lengthlike_body_widen` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/param_lengthlike_body_widen.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/param_lengthlike_body_widen.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:length` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'length' for nil (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
0
5
4
2
false
true

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'length' for nil (NoMethodError)
```

---

## `poly_keyed_hash_method_dedup` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/poly_keyed_hash_method_dedup.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/poly_keyed_hash_method_dedup.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:foo` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'foo' for class 'Class' (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (4 lines)
true
false
1
true

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'foo' for class 'Class' (NameError)
```

---

## `poly_keyed_hash_pipeline` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/poly_keyed_hash_pipeline.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/poly_keyed_hash_pipeline.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:read` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'read' for class 'Class' (NameError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
2
4

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'read' for class 'Class' (NameError)
```

---

## `primitive_eq_user_class` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/primitive_eq_user_class.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/primitive_eq_user_class.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 45
--- expected (49 lines)
false
false
false
false
false
false
false
false
false
false
false
false
false
false
false
false
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
false
false
false
false
false
true
false
false
true
done

--- actual (49 lines)
false
false
false
false
false
false
false
false
false
false
false
false
false
false
false
false
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
false
false
false
false
false
false
false
false
true
done
```

---

## `proc_capture_enclosing_lambda` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/proc_capture_enclosing_lambda.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/proc_capture_enclosing_lambda.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (11 lines)
5
7
11
12
101
13
"id:5"
7
[1, {x: 2}]
[1, {}]
3

--- actual (11 lines)
5
7
nil
nil
nil
nil
"id:5"
7
[1, {x: 2}]
[1, {}]
3
```

---

## `proc_cell_opassign_returns_new_value` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/proc_cell_opassign_returns_new_value.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/proc_cell_opassign_returns_new_value.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
15
15
6
6
3

--- actual (5 lines)
nil
15
nil
6
3
```

---

## `range_bsearch_float` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/range_bsearch_float.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/range_bsearch_float.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 4
--- expected (7 lines)
1.0000000000000002
4.0
nil
0.0
4
nil
6

--- actual (7 lines)
1.0000000000000002
4.0
nil
-0.0
4
nil
6
```

---

## `rational_complex_wave9` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/rational_complex_wave9.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/rational_complex_wave9.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-expected-numeric-value-got-ni-4730` (cluster `?`)

stderr:
```

thread '<unnamed>' (101109395) panicked at crates/zeo-rt/src/builtins/numeric.rs:73:18:
expected a numeric value, got 2+3i
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

diff:
```
=== stdout diff ===
first difference at line 15
--- expected (30 lines)
3
4
-4
-3
3
(31/10)
(16/5)
true
true
true
[(2/1), (1/2)]
0.816496580927726
(2+3i)
pow_float ok
(2+3i)
0.7853981633974483
0.7853981633974483
[3.605551275463989, 0.982793723247329]
[2, 3]
[2, 3]
(2+3i)
(2+3i)
2
(2+3i)
1
(1.0+1.5i)
[(2+0i), (2+3i)]
2.0
(5/2)
"RangeError"

--- actual (25 lines)
3
4
-4
-3
3
(31/10)
(16/5)
true
true
true
[(2/1), (1/2)]
0.816496580927726
(2+3i)
pow_float ok
(2.0+3.0i)
0.7853981633974483
0.7853981633974483
[3.605551275463989, 0.982793723247329]
[2, 3]
[2, 3]
(2+3i)
(2+3i)
2
(2+3i)
1

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (4 lines)

thread '<unnamed>' (101109395) panicked at crates/zeo-rt/src/builtins/numeric.rs:73:18:
expected a numeric value, got 2+3i
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

---

## `rational_i_to_complex` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/rational_i_to_complex.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/rational_i_to_complex.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (8 lines)
(0+0.75i)
(0-0.5i)
(0+2.0i)
Complex
0
0.75
true
true

--- actual (8 lines)
(0+(3/4)*i)
(0+(-1/2)*i)
(0+(2/1)*i)
Complex
0
(3/4)
true
true
```

---

## `rational_mod_cmp_slice_default_wave10` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/rational_mod_cmp_slice_default_wave10.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/rational_mod_cmp_slice_default_wave10.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (20 lines)
"ll"
"ell"
"ll"
"heo"
9
9
"d"
5
(1/6)
(3/2)
(1/6)
(1/6)
(1/6)
(-1/6)
[10, (1/6)]
0
1
-1
-1
nil

--- actual (20 lines)
nil
"ell"
nil
"hello"
9
9
"d"
5
(1/6)
(3/2)
(1/6)
(1/6)
0.16666666666666696
-0.16666666666666696
[(21/2), (1/6)]
0
nil
nil
nil
nil
```

---

## `regexp_brace_zero_lower` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/regexp_brace_zero_lower.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/regexp_brace_zero_lower.rb.expected`
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

- source: `/Users/ryanseys/dev/spinel/test/regexp_gsub_zero_width.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/regexp_gsub_zero_width.rb.expected`
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

- source: `/Users/ryanseys/dev/spinel/test/regexp_inline_options.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/regexp_inline_options.rb.expected`
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

## `regexp_line_anchors` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/regexp_line_anchors.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/regexp_line_anchors.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-error-unrecognized-escape-sequence-regexperror-2794` (cluster `?`)

stderr:
```
uncaught exception: regex parse error:
    a\Z
     ^^
error: unrecognized escape sequence (RegexpError)
```

diff:
```
=== stdout diff ===
first difference at line 14
--- expected (15 lines)
"FontName"
"Times"
["a", "b", "c"]
["a", "b", "c"]
[["foo", "1"], ["bar", "2"]]
["Name", "Lang", "Year"]
"Ada"
"Ruby"
"1995"
true
false
false
false
true
true

--- actual (13 lines)
"FontName"
"Times"
["a", "b", "c"]
["a", "b", "c"]
[["foo", "1"], ["bar", "2"]]
["Name", "Lang", "Year"]
"Ada"
"Ruby"
"1995"
true
false
false
false

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (4 lines)
uncaught exception: regex parse error:
    a\Z
     ^^
error: unrecognized escape sequence (RegexpError)
```

---

## `regexp_pike_capture_and_bol` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/regexp_pike_capture_and_bol.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/regexp_pike_capture_and_bol.rb.expected`
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

## `regexp_sub_gsub_realloc` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/regexp_sub_gsub_realloc.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/regexp_sub_gsub_realloc.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 5
--- expected (6 lines)
200
"zzzzzzzz"
200
200
500
true

--- actual (6 lines)
200
"zzzzzzzz"
200
200
100
false
```

---

## `send_literal_and_user` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/send_literal_and_user.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/send_literal_and_user.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:hi` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'hi' for an instance of Mailer (NoMethodError)
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
uncaught exception: undefined method 'hi' for an instance of Mailer (NoMethodError)
```

---

## `set_full_api` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/set_full_api.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/set_full_api.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 30
--- expected (40 lines)
[2, 3]
[1, 3]
2
2
6
1
3
6
{true => [1, 3], false => [2]}
[10, 30]
true
true
false
false
Set[1, 2, 3]
nil
Set[1, 3]
nil
Set[1, 2, 3, 4]
Set[1]
Set[1, 4]
true
true
false
true
true
true
true
true
-1
1
0
nil
Set[]
{true => Set[1, 3], false => Set[2]}
Set[Set[1, 3], Set[2]]
Set[1, 2, 3]
Set[1, 2, 3]
Set[1, 2, 3, 4]
Set["a", "b"]

--- actual (40 lines)
[2, 3]
[1, 3]
2
2
6
1
3
6
{true => [1, 3], false => [2]}
[10, 30]
true
true
false
false
Set[1, 2, 3]
nil
Set[1, 3]
nil
Set[1, 2, 3, 4]
Set[1]
Set[1, 4]
true
true
false
true
true
true
true
true
nil
nil
0
nil
Set[]
{true => Set[1, 3], false => Set[2]}
Set[Set[1, 3], Set[2]]
Set[1, 2, 3]
Set[1, 2, 3]
Set[1, 2, 3, 4]
Set["a", "b"]
```

---

## `signal_module_surface` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/signal_module_surface.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/signal_module_surface.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:kill` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'kill' for module Process (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 30
--- expected (37 lines)
2
0
15
Hash
"INT"
nil
"EXIT"
"CHLD"
"DEFAULT"
"IGNORE"
ArgumentError
Errno::EINVAL
ArgumentError
Proc
Signal
Module
"constant"
"SIGINT"
2
"SIGINT"
SignalException
"SIGTERM"
"SIGINT"
"Interrupt"
2
"stop"
ArgumentError
Interrupt
SignalException
1
Errno::ESRCH
handler true
main
"SYSTEM_DEFAULT"
Proc
1
1

--- actual (29 lines)
2
0
15
Hash
"INT"
nil
"EXIT"
"CHLD"
"DEFAULT"
"IGNORE"
ArgumentError
Errno::EINVAL
ArgumentError
Proc
Signal
Module
"constant"
"SIGINT"
2
"SIGINT"
SignalException
"SIGTERM"
"SIGINT"
"Interrupt"
2
"stop"
ArgumentError
Interrupt
SignalException

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'kill' for module Process (NoMethodError)
```

---

## `socket_tcp_thread` — TIMEOUT_RUN

- source: `/Users/ryanseys/dev/spinel/test/socket_tcp_thread.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/socket_tcp_thread.rb.expected`
- expected stderr: *(must be empty)*
- verdict: TIMEOUT_RUN (stage `run`) · bucket `-` (cluster `-`)

---

## `source_file` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/source_file.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/source_file.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (8 lines)
test/source_file.rb
true
[test/source_file.rb]
got: test/source_file.rb
true
true
true
param: test/source_file.rb

--- actual (8 lines)
/Users/ryanseys/dev/spinel/test/source_file.rb
true
[/Users/ryanseys/dev/spinel/test/source_file.rb]
got: /Users/ryanseys/dev/spinel/test/source_file.rb
true
true
true
param: /Users/ryanseys/dev/spinel/test/source_file.rb
```

---

## `sp_crypto_basic` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/sp_crypto_basic.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/sp_crypto_basic.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module Crypto (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (6 lines)
5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
aGVsbG8
hello
Eg-2z_z4syxD5yJSVsT4N6hlSMkszDVICAWYfLcL4Xs
22
diff

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module Crypto (NoMethodError)
```

---

## `sp_crypto_sha1` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/sp_crypto_sha1.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/sp_crypto_sha1.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module Crypto (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
a9993e364706816aba3e25717850c26c9cd0d89d
da39a3ee5e6b4b0d3255bfef95601890afd80709
s3pPLMBiTxaQ9kYGzzhZRbK+xOo=

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module Crypto (NoMethodError)
```

---

## `sp_net_basic` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/sp_net_basic.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/sp_net_basic.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:ffi_func` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'ffi_func' for module Net (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (2 lines)
hello
pid-ok

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module Net (NoMethodError)
```

---

## `str_method_nil_arg_no_segv` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/str_method_nil_arg_no_segv.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/str_method_nil_arg_no_segv.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `arity-panic` (cluster `g`)

stderr:
```
uncaught exception: wrong number of arguments (given 0, expected 1+) (ArgumentError)
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
uncaught exception: wrong number of arguments (given 0, expected 1+) (ArgumentError)
```

---

## `str_unpack_gc_root` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/str_unpack_gc_root.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/str_unpack_gc_root.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
["11223344", "55667788"]
40000
20000

--- actual (3 lines)
["11223344", "667788c2"]
40000
0
```

---

## `string_append_binary_safe` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_append_binary_safe.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_append_binary_safe.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (12 lines)
2
2
3
[126, 0, 200]
4
[120, 126, 0, 200]
134
0
130
5
6
2

--- actual (12 lines)
2
2
4
[126, 0, 195, 136]
5
[120, 126, 0, 195, 136]
136
126
0
5
6
2
```

---

## `string_conformance_batch4` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_conformance_batch4.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_conformance_batch4.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 5
--- expected (12 lines)
true
false
MatchData
NilClass
[92]
"[a]b"
"a\\b"
"\"\\u0002\""
"\"\\e\""
"\"\\a\\b\\t\\n\\v\\f\\r\""
"\"\\u0000\\u001C\\u007F\""
RangeError

--- actual (12 lines)
true
false
MatchData
NilClass
[92, 92]
"[\\&]b"
"a\\\\b"
"\"\\u0002\""
"\"\\e\""
"\"\\a\\b\\t\\n\\v\\f\\r\""
"\"\\u0000\\u001C\\u007F\""
RangeError
```

---

## `string_enum_arg_forms` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_enum_arg_forms.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_enum_arg_forms.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `arity-panic` (cluster `g`)

stderr:
```
uncaught exception: wrong number of arguments (given 1, expected 2) (ArgumentError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
["a", "a", "a"]
["1", "2"]
["1-", "2-", "3"]
["1-", "2-", "3"]
["x,", "y"]

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: wrong number of arguments (given 1, expected 2) (ArgumentError)
```

---

## `string_enum_block_returns_self` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_enum_block_returns_self.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_enum_block_returns_self.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (11 lines)
abc
abc
294
abc
195
ab
2
x
y
["a", "b", "c"]
[97, 98, 99]

--- actual (16 lines)

a
b
c
0
97
98
99
0
97
98
0
x
y
["a", "b", "c"]
[97, 98, 99]
```

---

## `string_enum_inspect_source` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_enum_inspect_source.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_enum_inspect_source.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-no-block-given-yield-localjumperror-1be0` (cluster `?`)

stderr:
```
uncaught exception: no block given (yield) (LocalJumpError)
```

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (8 lines)
#<Enumerator: "abc":each_char>
#<Enumerator: "a\nb\n":each_line>
#<Enumerator: "ab":each_byte>
#<Enumerator: "ab":each_codepoint>
#<Enumerator: "x\ny\n":each_line(chomp: true)>
"h"
"i"
["h", "i"]

--- actual (2 lines)
#<Enumerator: "abc":each_char>
#<Enumerator: "a\nb\n":each_line>

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: no block given (yield) (LocalJumpError)
```

---

## `string_equal_identity` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_equal_identity.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_equal_identity.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 8
--- expected (11 lines)
false
true
true
false
true
1
false
true
true
false
true

--- actual (11 lines)
false
true
true
false
true
1
false
false
true
false
false
```

---

## `string_oct_parse` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_oct_parse.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_oct_parse.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 16
--- expected (19 lines)
511
15
-10
15
8
31
15
3
99
3
65535
3
0
15
1
1
0
0
0

--- actual (19 lines)
511
15
-10
15
8
31
15
3
99
3
65535
3
0
15
1
8
1
1
0
```

---

## `string_plus_heap_gc` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_plus_heap_gc.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_plus_heap_gc.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:>` (cluster `P`)

stderr:
```
uncaught exception: undefined method '>' for nil (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
collected

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method '>' for nil (NoMethodError)
```

---

## `string_scrub` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_scrub.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_scrub.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 4
--- expected (4 lines)
?
a?b
valid
[239, 191, 189]

--- actual (4 lines)
?
a?b
valid
[63]
```

---

## `string_splice_family` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_splice_family.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_splice_family.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (15 lines)
["XYZlo world", "XYZlo world", true]
["h--ello", "h--ello", true]
["ho", "ho", true]
["helLO", "helLO", true]
["h!", "h!", true]
["seed", "seed", true]
"JELLO"
"IndexError: negative length -1"
"IndexError: index 5 out of string"
"IndexError: index -9 out of string"
["12", "abcd"]
[nil, "no digits"]
["12", "ab34"]
["user", "example"]
"v.2.3"

--- actual (15 lines)
["XYZ", "XYZlo world", false]
["--", "h--ello", false]
["", "ho", false]
["LO", "helLO", false]
["!", "h!", false]
["seed", "seed", true]
"J"
"IndexError: index 0 out of string"
"IndexError: index 5 out of string"
"IndexError: index -6 out of string"
[nil, "ab12cd"]
[nil, "no digits"]
[nil, "12ab34"]
[nil, nil]
"v1.2.3"
```

---

## `string_split_inline_arg_gc_root` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_split_inline_arg_gc_root.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_split_inline_arg_gc_root.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:<=` (cluster `P`)

stderr:
```
uncaught exception: undefined method '<=' for nil (NoMethodError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (1 lines)
ok

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method '<=' for nil (NoMethodError)
```

---

## `string_succ_scan_slice_tilde_wave10` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_succ_scan_slice_tilde_wave10.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_succ_scan_slice_tilde_wave10.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 20
--- expected (30 lines)
"<<koalb>>"
"ba"
"aaa"
"b0"
"AAa"
"aaa0"
"2.0"
"-10"
"abd"
"THX1139"
"**+"
"2000"
[["1"], ["2"]]
["1", "2"]
[["a", "1"], ["b", "2"]]
["1"]
["2"]
["a", "1"]
["b", "2"]
"ll"
"heo"
nil
"hello"
MatchData
true
"ll"
"l"
"he"
"o"
nil

--- actual (30 lines)
"<<koalb>>"
"ba"
"aaa"
"b0"
"AAa"
"aaa0"
"2.0"
"-10"
"abd"
"THX1139"
"**+"
"2000"
[["1"], ["2"]]
["1", "2"]
[["a", "1"], ["b", "2"]]
["1"]
["2"]
["a", "1"]
["b", "2"]
nil
"hello"
nil
"hello"
MatchData
true
"ll"
"l"
"he"
"o"
nil
```

---

## `string_with_embedded_nul` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/string_with_embedded_nul.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/string_with_embedded_nul.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 5
--- expected (16 lines)
1
1
0
2
2
0
200
3
120
0
121
4
0
200
4
200

--- actual (16 lines)
1
1
0
2
3
0
200
3
120
0
121
4
0
200
4
200
```

---

## `strip_nul` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/strip_nul.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/strip_nul.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (8 lines)
[104, 105]
[104, 105, 0]
[0, 104, 105]
[104, 0, 105]
"hi"
"x"
"ab"
"ab"

--- actual (8 lines)
[0, 104, 105, 0]
[0, 104, 105, 0]
[0, 104, 105, 0]
[0, 0, 104, 0, 105, 0]
"hi"
"x"
"ab"
"ab"
```

---

## `struct_block_constant_init` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/struct_block_constant_init.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/struct_block_constant_init.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-const:Line` (cluster `P`)

stderr:
```
uncaught exception: uninitialized constant Line (NameError)
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
uncaught exception: uninitialized constant Line (NameError)
```

---

## `struct_to_h_block` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/struct_to_h_block.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/struct_to_h_block.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (10 lines)
Alice
30
2
6
8
3
4
widget
7
true

--- actual (10 lines)


2
```

---

## `symbol_nil_bool_float_batch` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/symbol_nil_bool_float_batch.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/symbol_nil_bool_float_batch.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-can-coerce-complex-into-float-typeerror-9cc0` (cluster `?`)

stderr:
```
uncaught exception: can't coerce Complex into Float (TypeError)
```

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (32 lines)
:hello
:hello
true
"hello"
nil
nil
-1
true
true
true
true
false
true
true
TrueClass
TrueClass
FalseClass
NilClass
Integer
ArgumentError
ArgumentError
FloatDomainError
FloatDomainError
3
[0.5, 1.0]
[1.0, 2.0]
[2.0, 1.0]
0
nil
(0/1)
nil
true

--- actual (25 lines)
:hello
:hello
false
"hello"
nil
nil
-1
true
true
true
true
false
true
true
TrueClass
TrueClass
FalseClass
NilClass
Integer
ArgumentError
ArgumentError
FloatDomainError
FloatDomainError
3
[0.5, 1.0]

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: can't coerce Complex into Float (TypeError)
```

---

## `thread_pass_fairness` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/thread_pass_fairness.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/thread_pass_fairness.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (3 lines)
[:main, 0, :main, 1, :main, 2, 3, 4]
main_turns: 4
done

--- actual (3 lines)
[:main, :main, :main, 0, 1, 2, 3, 4]
main_turns: 4
done
```

---

## `toplevel_include_module_function` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/toplevel_include_module_function.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/toplevel_include_module_function.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `missing-method:hello` (cluster `P`)

stderr:
```
uncaught exception: undefined method 'hello' for main (NoMethodError)
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
uncaught exception: undefined method 'hello' for main (NoMethodError)
```

---

## `uniq_block` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/uniq_block.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/uniq_block.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (9 lines)
[1, 2]
[1, 2, 3]
3
["foo", "bar", "qux"]
[1.5, 2.5]
[1, "a"]
[]
[5, 6, 7]
[1, 2, 3]

--- actual (9 lines)
[1, 2, 3, 4]
[1, 2, 3, 4, 5, 6]
10
["foo", "bar", "baz", "qux"]
[1.5, 2.5, 3.5, 4.5]
[1, "a", 2, "b"]
[]
[5, 6, 7]
[1, 2, 3]
```

---

## `value_type_identity` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/value_type_identity.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/value_type_identity.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 3
--- expected (8 lines)
true
true
true
true
true
true
true
true

--- actual (8 lines)
true
true
false
true
false
true
true
false
```

---

## `while_until_as_value` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/spinel/test/while_until_as_value.rb`
- expected stdout: `/Users/ryanseys/dev/spinel/test/while_until_as_value.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 7
--- expected (9 lines)
nil
3
nil
2
[1, nil, 2]
"nil"
[3, 3]
[nil, 2]
nil

--- actual (9 lines)
nil
3
nil
2
[1, nil, 2]
"nil"
[nil, 3]
[nil, 2]
nil
```

---

