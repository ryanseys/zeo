//! `Array` (CRuby array.c) -- stage B carries the rows migrated from the
//! old curated table (arg-type mismatches upgraded from silent fall-through
//! to CRuby's real TypeError); the Tier A breadth lands in stage E.

use crate::builtins::{arg_int, arity, block_or_enum, builtin_methods, recv_array};
use crate::RubyValue;

builtin_methods! {
    pub(crate) fn lookup;

    "[]" | "slice" => fn index(recv, args, _block) {
        arity!(args, 1..=2);
        let items = recv_array!(recv).lock().clone();
        let n = items.len() as i64;
        // `arr[start, len]`.
        if args.len() == 2 {
            let (start, len) = (arg_int!(args, 0), arg_int!(args, 1));
            let start = if start < 0 { start + n } else { start };
            if start < 0 || start > n || len < 0 {
                return Ok(RubyValue::Nil);
            }
            let end = (start + len).min(n);
            return Ok(RubyValue::Array(crate::array_new(
                items[start as usize..end as usize].to_vec(),
            )));
        }
        match &args[0] {
            RubyValue::Int(_) => {
                let i = arg_int!(args, 0);
                Ok(crate::array_get(recv_array!(recv), i))
            }
            // `arr[1..3]` -- Range slicing.
            RubyValue::Range(start, end, exclusive) => {
                let s = match start.as_deref() {
                    Some(RubyValue::Int(v)) => {
                        if *v < 0 { v + n } else { *v }
                    }
                    None => 0,
                    _ => return Ok(RubyValue::Nil),
                };
                let e = match end.as_deref() {
                    Some(RubyValue::Int(v)) => {
                        let v = if *v < 0 { v + n } else { *v };
                        if *exclusive { v - 1 } else { v }
                    }
                    None => n - 1,
                    _ => return Ok(RubyValue::Nil),
                };
                if s < 0 || s > n {
                    return Ok(RubyValue::Nil);
                }
                let e = e.min(n - 1);
                Ok(RubyValue::Array(crate::array_new(if e < s {
                    Vec::new()
                } else {
                    items[s as usize..=e as usize].to_vec()
                })))
            }
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            )),
        }
    }
    "[]=" => fn index_set(recv, args, _block) {
        arity!(args, 2..=3);
        // `arr[start, len] = val` / `arr[range] = val` -- CRuby's splice
        // (rb_ary_splice): the removed span is replaced by the VALUE's
        // `to_ary` coercion's elements (a plain Array as-is; an object
        // without `to_ary` inserts as one element). The expression value is
        // the object as written, never the coercion.
        if args.len() == 3 {
            let (start, len) = (arg_int!(args, 0), arg_int!(args, 1));
            array_splice(recv_array!(recv), start, len, &args[2])?;
            return Ok(args[2].clone());
        }
        if let RubyValue::Range(s, e, exclusive) = &args[0] {
            let n = crate::array_len(recv_array!(recv));
            let start = match s.as_deref() {
                Some(RubyValue::Int(v)) => if *v < 0 { v + n } else { *v },
                None => 0,
                _ => return Err(crate::dispatch::raise_error(
                    "TypeError",
                    "no implicit conversion of Range into Integer".to_string(),
                )),
            };
            let end = match e.as_deref() {
                Some(RubyValue::Int(v)) => if *v < 0 { v + n } else { *v },
                None => n - 1,
                _ => return Err(crate::dispatch::raise_error(
                    "TypeError",
                    "no implicit conversion of Range into Integer".to_string(),
                )),
            };
            let len = (end - start + if *exclusive { 0 } else { 1 }).max(0);
            array_splice(recv_array!(recv), start, len, &args[1])?;
            return Ok(args[1].clone());
        }
        let i = arg_int!(args, 0);
        match crate::array_set(recv_array!(recv), i, args[1].clone()) {
            Some(v) => Ok(v),
            None => Err(crate::dispatch::raise_error(
                "IndexError",
                format!("index {i} too small for array; minimum: -{}", crate::array_len(recv_array!(recv))),
            )),
        }
    }
    "<<" | "push" | "append" => fn push(recv, args, _block) {
        // `push` is variadic (0+ args) in real Ruby; `<<` is arity 1, but
        // the parser only ever emits it with one argument, so one row
        // serves both.
        let arr = recv_array!(recv);
        for a in args {
            crate::array_push(arr, a.clone());
        }
        Ok(recv.clone())
    }
    "length" | "size" => fn length(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(crate::array_len(recv_array!(recv))))
    }
    "include?" | "member?" => fn include_p(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(crate::array_include(recv_array!(recv), &args[0])))
    }
    "empty?" => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(crate::array_len(recv_array!(recv)) == 0))
    }
    "first" => fn first(recv, args, _block) {
        // `first(n)` is Enumerable's n-form (next ancestor in the chain
        // implements it) -- only the 0-arg head accessor lives here.
        if !args.is_empty() {
            return crate::builtins::enumerable::enumerable_send(recv, "first", args, None)
                .expect("Enumerable implements first(n)");
        }
        Ok(crate::array_get(recv_array!(recv), 0))
    }
    // `last`/`last(n)` mirror `pop`'s dual return: bare answers ONE element
    // (nil when empty), `last(n)` an ARRAY of up to the last n, in original
    // order (`n` past the length takes what's there; `n == 0` is `[]`).
    "last" => fn last(recv, args, _block) {
        arity!(args, 0..=1);
        let items = recv_array!(recv).lock();
        let Some(n) = count_arg(args)? else {
            return Ok(items.last().cloned().unwrap_or(RubyValue::Nil));
        };
        let at = items.len().saturating_sub(n);
        Ok(RubyValue::Array(crate::array_new(items[at..].to_vec())))
    }
    "to_a" => fn to_a(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "+" => fn plus(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Array(other) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Array",
                    crate::builtins::class_name_of(&args[0])
                ),
            ));
        };
        let mut out = recv_array!(recv).lock().clone();
        out.extend(other.lock().iter().cloned());
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "-" => fn minus(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Array(other) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Array",
                    crate::builtins::class_name_of(&args[0])
                ),
            ));
        };
        let exclude = other.lock().clone();
        let out = recv_array!(recv)
            .lock()
            .iter()
            .filter(|e| !exclude.iter().any(|x| e.rb_eq(x)))
            .cloned()
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `arr * n` repeats; `arr * "sep"` joins (real Ruby's dual form).
    "*" => fn times(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(n) => {
                if *n < 0 {
                    return Err(crate::dispatch::raise_error(
                        "ArgumentError",
                        "negative argument".to_string(),
                    ));
                }
                let base = recv_array!(recv).lock().clone();
                let mut out = Vec::with_capacity(base.len() * *n as usize);
                for _ in 0..*n {
                    out.extend(base.iter().cloned());
                }
                Ok(RubyValue::Array(crate::array_new(out)))
            }
            RubyValue::Str(_) => join(recv, args, None),
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            )),
        }
    }
    "&" => fn intersect(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Array(other) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Array",
                    crate::builtins::class_name_of(&args[0])
                ),
            ));
        };
        let keep = other.lock().clone();
        let mut out: Vec<RubyValue> = Vec::new();
        for e in recv_array!(recv).lock().iter() {
            if keep.iter().any(|x| e.rb_eq(x)) && !out.iter().any(|x| e.rb_eq(x)) {
                out.push(e.clone());
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // Variadic siblings of `&`/`-`: `intersection` keeps self's elements
    // present in EVERY argument (uniq'd); `difference` keeps self's elements
    // absent from ALL arguments (duplicates preserved, like `-`).
    "intersection" => fn intersection(recv, args, _block) {
        let others = set_op_args(args)?;
        let mut out: Vec<RubyValue> = Vec::new();
        for e in recv_array!(recv).lock().iter() {
            let in_all = others.iter().all(|o| o.iter().any(|x| e.rb_eq(x)));
            if in_all && !out.iter().any(|x| e.rb_eq(x)) {
                out.push(e.clone());
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "difference" => fn difference(recv, args, _block) {
        let others = set_op_args(args)?;
        let out: Vec<RubyValue> = recv_array!(recv)
            .lock()
            .iter()
            .filter(|e| !others.iter().any(|o| o.iter().any(|x| e.rb_eq(x))))
            .cloned()
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `|` is the BINARY operator (`a | b`); `union` is its variadic sibling
    // (`a.union(b, c, ...)`, zero args = a uniq'd copy of self). Both drop
    // later duplicates, keeping first-occurrence order.
    "|" => fn or(recv, args, _block) {
        arity!(args, 1);
        let others = set_op_args(args)?;
        Ok(RubyValue::Array(crate::array_new(union_of(recv_array!(recv), &others))))
    }
    "union" => fn union(recv, args, _block) {
        let others = set_op_args(args)?;
        Ok(RubyValue::Array(crate::array_new(union_of(recv_array!(recv), &others))))
    }
    "<=>" => fn spaceship(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Array(other) = &args[0] else {
            return Ok(RubyValue::Nil);
        };
        let (a, b) = (recv_array!(recv).lock().clone(), other.lock().clone());
        for (x, y) in a.iter().zip(b.iter()) {
            match x.rb_cmp(y) {
                Some(0) => continue,
                Some(c) => return Ok(RubyValue::Int(c)),
                None => return Ok(RubyValue::Nil),
            }
        }
        Ok(RubyValue::Int((a.len() as i64 - b.len() as i64).signum()))
    }
    "==" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    // `pop`/`shift` answer ONE element (nil when empty); `pop(n)`/`shift(n)`
    // answer an ARRAY of up to n -- a different return type, not just a
    // different count, which is why the no-arg case can't just be `pop(1)`.
    // `n` past the length takes what's there; `n == 0` is `[]`.
    "pop" => fn pop(recv, args, _block) {
        arity!(args, 0..=1);
        let handle = recv_array!(recv);
        let mut guard = handle.lock();
        let Some(n) = count_arg(args)? else {
            return Ok(guard.pop().unwrap_or(RubyValue::Nil));
        };
        let at = guard.len().saturating_sub(n);
        let taken: Vec<RubyValue> = guard.split_off(at);
        Ok(RubyValue::Array(crate::array_new(taken)))
    }
    "shift" => fn shift(recv, args, _block) {
        arity!(args, 0..=1);
        let handle = recv_array!(recv);
        let mut guard = handle.lock();
        let Some(n) = count_arg(args)? else {
            return Ok(if guard.is_empty() {
                RubyValue::Nil
            } else {
                guard.remove(0)
            });
        };
        let n = n.min(guard.len());
        let rest = guard.split_off(n);
        let taken = std::mem::replace(&mut *guard, rest);
        Ok(RubyValue::Array(crate::array_new(taken)))
    }
    "unshift" | "prepend" => fn unshift(recv, args, _block) {
        let handle = recv_array!(recv);
        let mut guard = handle.lock();
        for (i, a) in args.iter().enumerate() {
            guard.insert(i, a.clone());
        }
        drop(guard);
        Ok(recv.clone())
    }
    "concat" => fn concat(recv, args, _block) {
        for a in args {
            let RubyValue::Array(other) = a else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into Array",
                        crate::builtins::class_name_of(a)
                    ),
                ));
            };
            let extension = other.lock().clone();
            recv_array!(recv).lock().extend(extension);
        }
        Ok(recv.clone())
    }
    // `flatten` / `flatten(depth)`.
    "flatten" => fn flatten(recv, args, _block) {
        arity!(args, 0..=1);
        let depth = match args.first() {
            Some(RubyValue::Int(d)) => *d,
            None | Some(RubyValue::Nil) => -1,
            Some(other) => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into Integer",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        };
        fn go(items: &[RubyValue], depth: i64, out: &mut Vec<RubyValue>) {
            for e in items {
                match e {
                    RubyValue::Array(inner) if depth != 0 => {
                        let inner = inner.lock().clone();
                        go(&inner, depth - 1, out);
                    }
                    other => out.push(other.clone()),
                }
            }
        }
        let items = recv_array!(recv).lock().clone();
        let mut out = Vec::new();
        go(&items, depth, &mut out);
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "compact" => fn compact(recv, args, _block) {
        arity!(args, 0);
        let out = recv_array!(recv)
            .lock()
            .iter()
            .filter(|e| !e.is_nil())
            .cloned()
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "uniq" => fn uniq(recv, args, _block) {
        arity!(args, 0);
        let mut out: Vec<RubyValue> = Vec::new();
        for e in recv_array!(recv).lock().iter() {
            if !out.iter().any(|x| e.rb_eq(x)) {
                out.push(e.clone());
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "uniq!" => fn uniq_bang(recv, args, _block) {
        arity!(args, 0);
        let h = recv_array!(recv);
        let mut out: Vec<RubyValue> = Vec::new();
        let before = h.lock().len();
        for e in h.lock().iter() {
            if !out.iter().any(|x| e.rb_eq(x)) {
                out.push(e.clone());
            }
        }
        if out.len() == before {
            return Ok(RubyValue::Nil); // no change -- CRuby's nil answer
        }
        *h.lock() = out;
        Ok(recv.clone())
    }
    "assoc" => fn assoc(recv, args, _block) {
        arity!(args, 1);
        for e in recv_array!(recv).lock().iter() {
            if let RubyValue::Array(inner) = e {
                if inner.lock().first().is_some_and(|k| k.rb_eq(&args[0])) {
                    return Ok(e.clone());
                }
            }
        }
        Ok(RubyValue::Nil)
    }
    "rassoc" => fn rassoc(recv, args, _block) {
        arity!(args, 1);
        for e in recv_array!(recv).lock().iter() {
            if let RubyValue::Array(inner) = e {
                if inner.lock().get(1).is_some_and(|v| v.rb_eq(&args[0])) {
                    return Ok(e.clone());
                }
            }
        }
        Ok(RubyValue::Nil)
    }
    "product" => fn product(recv, args, _block) {
        // Cartesian product of self with every argument array, CRuby's
        // element order (leftmost varies slowest).
        let mut lists: Vec<Vec<RubyValue>> = vec![recv_array!(recv).lock().clone()];
        for a in args {
            let RubyValue::Array(other) = a else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("no implicit conversion of {} into Array", a.inspect_string()),
                ));
            };
            lists.push(other.lock().clone());
        }
        let mut out: Vec<RubyValue> = vec![RubyValue::Array(crate::array_new(Vec::new()))];
        let mut tuples: Vec<Vec<RubyValue>> = vec![Vec::new()];
        for list in &lists {
            let mut next = Vec::with_capacity(tuples.len() * list.len());
            for t in &tuples {
                for e in list {
                    let mut t2 = t.clone();
                    t2.push(e.clone());
                    next.push(t2);
                }
            }
            tuples = next;
        }
        out.clear();
        out.extend(tuples.into_iter().map(|t| RubyValue::Array(crate::array_new(t))));
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "transpose" => fn transpose(recv, args, _block) {
        arity!(args, 0);
        let rows = recv_array!(recv).lock().clone();
        if rows.is_empty() {
            return Ok(RubyValue::Array(crate::array_new(Vec::new())));
        }
        let mut cols: Vec<Vec<RubyValue>> = Vec::new();
        for (ri, row) in rows.iter().enumerate() {
            let RubyValue::Array(r) = row else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("no implicit conversion of {} into Array", row.inspect_string()),
                ));
            };
            let r = r.lock().clone();
            if ri == 0 {
                cols = vec![Vec::with_capacity(rows.len()); r.len()];
            } else if r.len() != cols.len() {
                return Err(crate::dispatch::raise_error(
                    "IndexError",
                    format!("element size differs ({} should be {})", r.len(), cols.len()),
                ));
            }
            for (ci, v) in r.into_iter().enumerate() {
                cols[ci].push(v);
            }
        }
        Ok(RubyValue::Array(crate::array_new(
            cols.into_iter().map(|c| RubyValue::Array(crate::array_new(c))).collect(),
        )))
    }
    "slice!" => fn slice_bang(recv, args, _block) {
        // `slice!(i)` / `slice!(i, len)` -- remove and return. The Range
        // form isn't wired yet (TODO(plan G6)).
        let h = recv_array!(recv);
        let len = h.lock().len() as i64;
        let i = arg_int!(args, 0);
        let idx = if i < 0 { i + len } else { i };
        if idx < 0 || idx > len {
            return Ok(RubyValue::Nil);
        }
        match args.len() {
            1 => {
                if idx == len {
                    return Ok(RubyValue::Nil);
                }
                Ok(h.lock().remove(idx as usize))
            }
            2 => {
                let n = arg_int!(args, 1).max(0);
                let end = ((idx + n) as usize).min(len as usize);
                let removed: Vec<RubyValue> = h.lock().drain(idx as usize..end).collect();
                Ok(RubyValue::Array(crate::array_new(removed)))
            }
            n => Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("wrong number of arguments (given {n}, expected 1..2)"),
            )),
        }
    }
    "reverse" => fn reverse(recv, args, _block) {
        arity!(args, 0);
        let mut out = recv_array!(recv).lock().clone();
        out.reverse();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "reverse!" => fn reverse_bang(recv, args, _block) {
        arity!(args, 0);
        recv_array!(recv).lock().reverse();
        Ok(recv.clone())
    }
    "join" => fn join(recv, args, _block) {
        arity!(args, 0..=1);
        let sep = match args.first() {
            Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
            None | Some(RubyValue::Nil) => String::new(),
            Some(other) => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        };
        let parts: Vec<String> = recv_array!(recv)
            .lock()
            .iter()
            .map(|e| e.to_display_string())
            .collect();
        Ok(RubyValue::Str(crate::string_new(parts.join(&sep))))
    }
    "index" | "find_index" => fn find_index(recv, args, block) {
        let items = recv_array!(recv).lock().clone();
        if let Some(RubyValue::Proc(p)) = &block {
            for (i, e) in items.iter().enumerate() {
                if p.call(std::slice::from_ref(e))?.truthy() {
                    return Ok(RubyValue::Int(i as i64));
                }
            }
            return Ok(RubyValue::Nil);
        }
        arity!(args, 1);
        Ok(match items.iter().position(|e| e.rb_eq(&args[0])) {
            Some(i) => RubyValue::Int(i as i64),
            None => RubyValue::Nil,
        })
    }
    // `rindex(obj)` matches by `==` from the right; `rindex { |e| }` finds
    // the last element the block answers truthy for.
    "rindex" => fn rindex(recv, args, block) {
        let items = recv_array!(recv).lock().clone();
        if let Some(RubyValue::Proc(p)) = &block {
            for i in (0..items.len()).rev() {
                if p.call(std::slice::from_ref(&items[i]))?.truthy() {
                    return Ok(RubyValue::Int(i as i64));
                }
            }
            return Ok(RubyValue::Nil);
        }
        arity!(args, 1);
        Ok(match items.iter().rposition(|e| e.rb_eq(&args[0])) {
            Some(i) => RubyValue::Int(i as i64),
            None => RubyValue::Nil,
        })
    }
    "dig" => fn dig(recv, args, _block) {
        if args.is_empty() {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "wrong number of arguments (given 0, expected 1+)".to_string(),
            ));
        }
        let mut cur = index_only(recv, &args[0])?;
        for key in &args[1..] {
            if cur.is_nil() {
                return Ok(RubyValue::Nil);
            }
            // Each level digs via its OWN class's [] (Hash keys, Array
            // indices) -- one dynamic dispatch per level, real Ruby's rule.
            cur = crate::dispatch::send_value(
                &cur,
                crate::Symbol::intern("[]"),
                std::slice::from_ref(key),
                None,
            )?;
        }
        Ok(cur)
    }
    "fetch" => fn fetch(recv, args, block) {
        arity!(args, 1..=2);
        let i = arg_int!(args, 0);
        let items = recv_array!(recv).lock().clone();
        let n = items.len() as i64;
        let idx = if i < 0 { i + n } else { i };
        if (0..n).contains(&idx) {
            return Ok(items[idx as usize].clone());
        }
        if let Some(default) = args.get(1) {
            return Ok(default.clone());
        }
        if let Some(RubyValue::Proc(p)) = &block {
            return p.call(&[RubyValue::Int(i)]);
        }
        Err(crate::dispatch::raise_error(
            "IndexError",
            format!("index {i} outside of array bounds: {}...{n}", -n),
        ))
    }
    "delete" => fn delete(recv, args, _block) {
        arity!(args, 1);
        let handle = recv_array!(recv);
        let mut guard = handle.lock();
        let before = guard.len();
        guard.retain(|e| !e.rb_eq(&args[0]));
        Ok(if guard.len() < before {
            args[0].clone()
        } else {
            RubyValue::Nil
        })
    }
    "delete_at" => fn delete_at(recv, args, _block) {
        arity!(args, 1);
        let i = arg_int!(args, 0);
        let handle = recv_array!(recv);
        let mut guard = handle.lock();
        let n = guard.len() as i64;
        let idx = if i < 0 { i + n } else { i };
        Ok(if (0..n).contains(&idx) {
            guard.remove(idx as usize)
        } else {
            RubyValue::Nil
        })
    }
    "insert" => fn insert(recv, args, _block) {
        if args.len() < 2 {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("wrong number of arguments (given {}, expected 2+)", args.len()),
            ));
        }
        let at = arg_int!(args, 0);
        let handle = recv_array!(recv);
        let mut guard = handle.lock();
        let n = guard.len() as i64;
        let at = if at < 0 { at + n + 1 } else { at };
        for (offset, v) in args[1..].iter().enumerate() {
            let pos = (at as usize + offset).min(guard.len());
            guard.insert(pos, v.clone());
        }
        drop(guard);
        Ok(recv.clone())
    }
    "zip" => fn zip(recv, args, block) {
        let base = recv_array!(recv).lock().clone();
        let others: Vec<Vec<RubyValue>> = args
            .iter()
            .map(|a| match a {
                RubyValue::Array(x) => Ok(x.lock().clone()),
                other => Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "wrong argument type {} (must respond to :each)",
                        crate::builtins::class_name_of(other)
                    ),
                )),
            })
            .collect::<Result<_, _>>()?;
        let out = base
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut row = vec![e.clone()];
                for o in &others {
                    row.push(o.get(i).cloned().unwrap_or(RubyValue::Nil));
                }
                RubyValue::Array(crate::array_new(row))
            })
            .collect();
        // The BLOCK form yields each row and answers nil, rather than
        // building the result array at all -- real Ruby's own rule
        // (`[1,2].zip([3,4]) { |r| }` is nil, oracle-checked).
        if let Some(b) = block {
            for row in out {
                b.as_proc_unchecked().call(&[row])?;
            }
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "rotate" => fn rotate(recv, args, _block) {
        arity!(args, 0..=1);
        let by = match args.first() {
            Some(RubyValue::Int(n)) => *n,
            None => 1,
            Some(other) => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into Integer",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        };
        let mut out = recv_array!(recv).lock().clone();
        if !out.is_empty() {
            let n = out.len() as i64;
            let by = by.rem_euclid(n) as usize;
            out.rotate_left(by);
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `values_at(0, 2, -1)` / `values_at(0..1, 3..)` -- each argument is an
    // index OR a range of them, and an out-of-bounds index contributes nil
    // rather than being skipped.
    //
    // A range past the end still contributes one entry PER INDEX it names,
    // which is the non-obvious part: on a 5-element array,
    // `values_at(3..9)` is `[4, 5, nil, nil, nil, nil, nil]` (7 entries,
    // for indices 3..9) -- oracle-verified. So the range is expanded to its
    // indices and each looked up exactly as a bare Int index would be,
    // rather than being clamped to the array like `arr[3..9]` slicing is.
    "values_at" => fn values_at(recv, args, _block) {
        let items = recv_array!(recv).lock().clone();
        let n = items.len() as i64;
        let at = |i: i64| {
            let idx = if i < 0 { i + n } else { i };
            if (0..n).contains(&idx) {
                items[idx as usize].clone()
            } else {
                RubyValue::Nil
            }
        };
        let mut out = Vec::new();
        for a in args {
            match a {
                RubyValue::Int(i) => out.push(at(*i)),
                RubyValue::Range(start, end, exclusive) => {
                    let s = match start.as_deref() {
                        Some(RubyValue::Int(v)) => if *v < 0 { v + n } else { *v },
                        None => 0,
                        _ => return Err(range_index_error(a)),
                    };
                    // An endless range stops at the array's end -- it names
                    // no index past it, unlike a bounded one.
                    let e = match end.as_deref() {
                        Some(RubyValue::Int(v)) => {
                            let v = if *v < 0 { v + n } else { *v };
                            if *exclusive { v - 1 } else { v }
                        }
                        None => n - 1,
                        _ => return Err(range_index_error(a)),
                    };
                    for i in s..=e {
                        out.push(at(i));
                    }
                }
                other => return Err(range_index_error(other)),
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "at" => fn at(recv, args, _block) {
        arity!(args, 1);
        let i = arg_int!(args, 0);
        Ok(crate::array_get(recv_array!(recv), i))
    }
    // `fill` writes a contiguous span, optionally growing the array.
    // Value form: `fill(obj[, start[, length]])`. Block form:
    // `fill([start[, length]]) { |i| }` -- the block maps each index to its
    // value. `start` is end-relative when negative; with no `length` the
    // span runs to the current end (so a `start` past the end is a no-op);
    // with `length`, `start + length` may extend past the end, back-filling
    // any gap with nil.
    "fill" => fn fill(recv, args, block) {
        let handle = recv_array!(recv);
        let cur_len = handle.lock().len() as i64;
        let (value, span): (Option<RubyValue>, &[RubyValue]) = match &block {
            Some(RubyValue::Proc(_)) => {
                arity!(args, 0..=2);
                (None, args)
            }
            _ => {
                arity!(args, 1..=3);
                (Some(args[0].clone()), &args[1..])
            }
        };
        let start = match span.first() {
            Some(v) => {
                let s = int_arg(v)?;
                if s < 0 { s + cur_len } else { s }
            }
            None => 0,
        };
        if start < 0 {
            return Err(crate::dispatch::raise_error(
                "IndexError",
                format!("index {} too small for array; minimum: {}", start - cur_len, -cur_len),
            ));
        }
        let end = match span.get(1) {
            Some(v) => start + int_arg(v)?.max(0),
            None => cur_len,
        };
        if end > cur_len {
            handle.lock().resize(end as usize, RubyValue::Nil);
        }
        for i in start..end {
            let v = match (&value, &block) {
                (Some(obj), _) => obj.clone(),
                // Compute the block's value WITHOUT holding the array lock --
                // the block may itself touch the array.
                (None, Some(RubyValue::Proc(p))) => p.call(&[RubyValue::Int(i)])?,
                _ => unreachable!("fill has either a value or a block"),
            };
            handle.lock()[i as usize] = v;
        }
        Ok(recv.clone())
    }
    "clear" => fn clear(recv, args, _block) {
        arity!(args, 0);
        recv_array!(recv).lock().clear();
        Ok(recv.clone())
    }
    "replace" => fn replace(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Array(other) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Array",
                    crate::builtins::class_name_of(&args[0])
                ),
            ));
        };
        let new_items = other.lock().clone();
        *recv_array!(recv).lock() = new_items;
        Ok(recv.clone())
    }
    // `sort` with rb_cmp or a comparator block; `sort!` in place.
    "sort" => fn sort(recv, args, block) {
        arity!(args, 0);
        let mut items = recv_array!(recv).lock().clone();
        sort_items(&mut items, &block)?;
        Ok(RubyValue::Array(crate::array_new(items)))
    }
    "sort!" => fn sort_bang(recv, args, block) {
        arity!(args, 0);
        let mut items = recv_array!(recv).lock().clone();
        sort_items(&mut items, &block)?;
        *recv_array!(recv).lock() = items;
        Ok(recv.clone())
    }
    "map!" | "collect!" => fn map_bang(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "map!", args, block);
        let items = recv_array!(recv).lock().clone();
        let mut out = Vec::with_capacity(items.len());
        for e in items {
            out.push(p.call(&[e])?);
        }
        *recv_array!(recv).lock() = out;
        Ok(recv.clone())
    }
    // In-place filters: self when anything changed, nil otherwise (real
    // Ruby's contract).
    "select!" | "filter!" => fn select_bang(recv, args, block) {
        arity!(args, 0);
        in_place_filter(recv, "select!", block, true)
    }
    "reject!" => fn reject_bang(recv, args, block) {
        arity!(args, 0);
        in_place_filter(recv, "reject!", block, false)
    }
    // keep_if/delete_if return SELF (not self-or-nil), so their blockless
    // Enumerator must short-circuit before the self return.
    "keep_if" => fn keep_if(recv, args, block) {
        arity!(args, 0);
        if !matches!(block, Some(RubyValue::Proc(_))) {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "keep_if", args));
        }
        in_place_filter(recv, "keep_if", block, true)?;
        Ok(recv.clone())
    }
    "delete_if" => fn delete_if(recv, args, block) {
        arity!(args, 0);
        if !matches!(block, Some(RubyValue::Proc(_))) {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "delete_if", args));
        }
        in_place_filter(recv, "delete_if", block, false)?;
        Ok(recv.clone())
    }
    // `sample` answers ONE random element (nil when empty); `sample(n)` an
    // ARRAY of up to n DISTINCT elements (a partial Fisher-Yates shuffle).
    // Shares Kernel#rand's PRNG (srand-reseedable; documented MT19937
    // divergence -- tests assert membership/length, not values).
    "sample" => fn sample(recv, args, _block) {
        arity!(args, 0..=1);
        let mut items = recv_array!(recv).lock().clone();
        let Some(n) = count_arg(args)? else {
            return Ok(if items.is_empty() {
                RubyValue::Nil
            } else {
                items[(crate::builtins::kernel::prng_next() % items.len() as u64) as usize].clone()
            });
        };
        let take = n.min(items.len());
        for i in 0..take {
            let j = i + (crate::builtins::kernel::prng_next() as usize % (items.len() - i));
            items.swap(i, j);
        }
        items.truncate(take);
        Ok(RubyValue::Array(crate::array_new(items)))
    }
    "shuffle" => fn shuffle(recv, args, _block) {
        arity!(args, 0);
        let mut items = recv_array!(recv).lock().clone();
        // Fisher-Yates over the shared PRNG.
        for i in (1..items.len()).rev() {
            let j = (crate::builtins::kernel::prng_next() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
        Ok(RubyValue::Array(crate::array_new(items)))
    }
    // With a block, each element is MAPPED to its pair first -- an Array
    // yields one value per element, so the block sees the element itself
    // (`[[1, 2]].to_h { |pair| }` gets `[1, 2]`; `{ |a, b| }` auto-splats).
    "to_h" => fn to_h(recv, args, block) {
        arity!(args, 0);
        let items = recv_array!(recv).lock().clone();
        let pairs = crate::builtins::enumerable::to_h_pairs(
            items.iter().map(|e| (std::slice::from_ref(e), e)),
            &block,
        )?;
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    "each" => fn each(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each", args, block);
        // Snapshot: mutating the array from inside the block iterates the
        // original elements (a deliberate, simpler rule than CRuby's
        // live-view semantics).
        let elems: Vec<RubyValue> = recv_array!(recv).lock().clone();
        for e in elems {
            p.call(&[e])?;
        }
        Ok(recv.clone())
    }
    "each_index" => fn each_index(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_index", args, block);
        let n = recv_array!(recv).lock().len();
        for i in 0..n {
            p.call(&[RubyValue::Int(i as i64)])?;
        }
        Ok(recv.clone())
    }
    // `combination(n)`/`permutation(n)`: the block form yields each tuple
    // and answers the receiver; the blockless form answers an Enumerator
    // (`block_or_enum!`), which is what makes the idiomatic
    // `.combination(2).to_a` work.
    //
    // Ruby's own edge cases, oracle-verified, and the reason the bounds
    // aren't just `n > len => empty`:
    //   [1,2,3].combination(0) => [[]]   -- ONE empty tuple, not none
    //   [1,2,3].combination(4) => []
    //   [1,2].permutation      => the FULL-length permutations (no arg)
    "combination" => fn combination(recv, args, block) {
        arity!(args, 1);
        let n = arg_int!(args, 0);
        let p = block_or_enum!(recv, "combination", args, block);
        let items = recv_array!(recv).lock().clone();
        for tuple in combinations_of(&items, n) {
            p.call(&[RubyValue::Array(crate::array_new(tuple))])?;
        }
        Ok(recv.clone())
    }
    "permutation" => fn permutation(recv, args, block) {
        arity!(args, 0..=1);
        let items = recv_array!(recv).lock().clone();
        // No argument means the receiver's own length -- read BEFORE
        // `block_or_enum!`'s early return so the Enumerator it builds
        // re-invokes with the identical (empty) argument list.
        let n = match args.first() {
            Some(_) => arg_int!(args, 0),
            None => items.len() as i64,
        };
        let p = block_or_enum!(recv, "permutation", args, block);
        for tuple in permutations_of(&items, n) {
            p.call(&[RubyValue::Array(crate::array_new(tuple))])?;
        }
        Ok(recv.clone())
    }
    // `cycle(n)` repeats the whole array n times; `cycle` with no argument
    // repeats FOREVER (so it only terminates via `break`) -- that infinite
    // form is why this can't just materialize the repeated array.
    // `cycle(0)`/`cycle(-1)` yield nothing at all.
    "cycle" => fn cycle(recv, args, block) {
        arity!(args, 0..=1);
        let count = match args.first() {
            None | Some(RubyValue::Nil) => None,
            Some(_) => Some(arg_int!(args, 0)),
        };
        let p = block_or_enum!(recv, "cycle", args, block);
        let items = recv_array!(recv).lock().clone();
        // An empty receiver never yields, and would spin forever below.
        if items.is_empty() {
            return Ok(RubyValue::Nil);
        }
        let mut remaining = count.unwrap_or(1);
        while remaining > 0 {
            for e in &items {
                p.call(&[e.clone()])?;
            }
            if count.is_some() {
                remaining -= 1;
            }
        }
        Ok(RubyValue::Nil)
    }
    // `bsearch`/`bsearch_index` in FIND-MINIMUM mode: the block answers
    // true/false and the array must be sorted so that every false precedes
    // every true; the answer is the first true (`nil` if there is none).
    // CRuby also has a find-any mode (the block answering an Integer);
    // that's a documented gap -- a numeric block result raises rather than
    // silently treating it as truthy and answering the wrong element.
    "bsearch" => fn bsearch(recv, args, block) {
        arity!(args, 0);
        let items = recv_array!(recv).lock().clone();
        Ok(match bsearch_find_min(&items, block)? {
            Some(i) => items[i].clone(),
            None => RubyValue::Nil,
        })
    }
    "bsearch_index" => fn bsearch_index(recv, args, block) {
        arity!(args, 0);
        let items = recv_array!(recv).lock().clone();
        Ok(match bsearch_find_min(&items, block)? {
            Some(i) => RubyValue::Int(i as i64),
            None => RubyValue::Nil,
        })
    }
    // `flatten!`/`sort_by!`: the bang forms mutate in place. `flatten!`
    // answers nil when NOTHING changed (Ruby's convention for the
    // destructive forms); `sort_by!` always answers the receiver.
    "flatten!" => fn flatten_bang(recv, args, _block) {
        arity!(args, 0..=1);
        let depth = match args.first() {
            None | Some(RubyValue::Nil) => -1,
            Some(_) => arg_int!(args, 0),
        };
        let cell = recv_array!(recv);
        let before = cell.lock().clone();
        let after = flatten_to_depth(&before, depth);
        if after.len() == before.len()
            && after.iter().zip(before.iter()).all(|(a, b)| a.rb_eq(b))
        {
            return Ok(RubyValue::Nil);
        }
        *cell.lock() = after;
        Ok(recv.clone())
    }
    "sort_by!" => fn sort_by_bang(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "sort_by!", args, block);
        let cell = recv_array!(recv);
        let items = cell.lock().clone();
        // Decorate-sort-undecorate: the block runs once per element, as
        // CRuby's does, rather than once per comparison.
        let mut keyed: Vec<(RubyValue, RubyValue)> = Vec::with_capacity(items.len());
        for e in items {
            keyed.push((p.call(&[e.clone()])?, e));
        }
        keyed.sort_by(|a, b| match a.0.rb_cmp(&b.0) {
            Some(o) => o.cmp(&0),
            None => std::cmp::Ordering::Equal,
        });
        *cell.lock() = keyed.into_iter().map(|(_, e)| e).collect();
        Ok(recv.clone())
    }
}

/// The optional COUNT argument of `pop(n)`/`shift(n)`: `None` when absent
/// (the answer-one-element form), else the count. A negative one is
/// CRuby's "negative array size" ArgumentError.
/// An Integer argument (`fill`'s start/length), raising CRuby's exact
/// TypeError for a non-Integer.
fn int_arg(v: &RubyValue) -> Result<i64, crate::Signal> {
    match v {
        RubyValue::Int(n) => Ok(*n),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

/// Coerces every argument of a variadic set op (`union`/`intersection`/
/// `difference`) to its element vector, raising CRuby's TypeError for a
/// non-Array argument.
fn set_op_args(args: &[RubyValue]) -> Result<Vec<Vec<RubyValue>>, crate::Signal> {
    args.iter()
        .map(|a| match a {
            RubyValue::Array(o) => Ok(o.lock().clone()),
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Array",
                    crate::builtins::class_name_of(other)
                ),
            )),
        })
        .collect()
}

/// The uniq'd concatenation `self ++ others...`, first occurrence winning --
/// shared by `|` (binary) and `union` (variadic).
fn union_of(recv: &crate::collections::RArray, others: &[Vec<RubyValue>]) -> Vec<RubyValue> {
    let mut out: Vec<RubyValue> = Vec::new();
    for e in recv.lock().iter().chain(others.iter().flatten()) {
        if !out.iter().any(|x| e.rb_eq(x)) {
            out.push(e.clone());
        }
    }
    out
}

fn count_arg(args: &[RubyValue]) -> Result<Option<usize>, crate::Signal> {
    let Some(v) = args.first() else {
        return Ok(None);
    };
    let RubyValue::Int(n) = v else {
        return Err(range_index_error(v));
    };
    if *n < 0 {
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            "negative array size".to_string(),
        ));
    }
    Ok(Some(*n as usize))
}

/// CRuby's TypeError for a `values_at` argument that is neither an index
/// nor a range of them.
fn range_index_error(v: &RubyValue) -> crate::Signal {
    crate::dispatch::raise_error(
        "TypeError",
        format!(
            "no implicit conversion of {} into Integer",
            crate::builtins::class_name_of(v)
        ),
    )
}

/// The INDEX of the first element the block answers truthy for, in
/// FIND-MINIMUM mode: `items` must be sorted so every false precedes every
/// true, and the search is the ordinary binary one over that boundary.
/// Shared by `bsearch` (which wants the element) and `bsearch_index`.
///
/// CRuby also has a find-any mode, selected by the block answering an
/// Integer instead of a boolean. That's a documented gap -- and it RAISES
/// rather than falling through, because a numeric result is truthy, so
/// treating it as the boolean mode would silently answer the wrong element
/// instead of failing.
fn bsearch_find_min(
    items: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<Option<usize>, crate::Signal> {
    let p = crate::builtins::need_block!(block);
    let (mut lo, mut hi) = (0usize, items.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let r = p.call(&[items[mid].clone()])?;
        match r {
            RubyValue::Int(_) | RubyValue::Float(_) => {
                return Err(crate::dispatch::raise_error(
                    "NotImplementedError",
                    "Array#bsearch's find-any mode (a numeric block result) isn't supported yet (spike scope)".to_string(),
                ))
            }
            _ if r.truthy() => hi = mid,
            _ => lo = mid + 1,
        }
    }
    Ok((lo < items.len()).then_some(lo))
}

/// Every `n`-element combination of `items`, in Ruby's order (indices
/// ascending, lexicographic). `n == 0` is ONE empty tuple; `n` past the
/// length, or negative, is none at all.
fn combinations_of(items: &[RubyValue], n: i64) -> Vec<Vec<RubyValue>> {
    if n < 0 || n as usize > items.len() {
        return Vec::new();
    }
    let n = n as usize;
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    let mut idx: Vec<usize> = (0..n).collect();
    loop {
        out.push(idx.iter().map(|&i| items[i].clone()).collect());
        // Advance the rightmost index that still has room, then repack the
        // ones after it -- the standard odometer over ascending indices.
        let Some(i) = (0..n).rev().find(|&i| idx[i] != i + items.len() - n) else {
            return out;
        };
        idx[i] += 1;
        for j in i + 1..n {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

/// Every `n`-element permutation of `items`, in Ruby's order. Same bounds
/// rules as `combinations_of`.
fn permutations_of(items: &[RubyValue], n: i64) -> Vec<Vec<RubyValue>> {
    if n < 0 || n as usize > items.len() {
        return Vec::new();
    }
    let n = n as usize;
    let mut out = Vec::new();
    let mut chosen: Vec<usize> = Vec::with_capacity(n);
    let mut used = vec![false; items.len()];
    walk_permutations(items, n, &mut chosen, &mut used, &mut out);
    out
}

fn walk_permutations(
    items: &[RubyValue],
    n: usize,
    chosen: &mut Vec<usize>,
    used: &mut Vec<bool>,
    out: &mut Vec<Vec<RubyValue>>,
) {
    if chosen.len() == n {
        out.push(chosen.iter().map(|&i| items[i].clone()).collect());
        return;
    }
    for i in 0..items.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        chosen.push(i);
        walk_permutations(items, n, chosen, used, out);
        chosen.pop();
        used[i] = false;
    }
}

/// Flattens nested arrays up to `depth` levels (`-1` = fully). Shared by
/// `flatten` and `flatten!`.
fn flatten_to_depth(items: &[RubyValue], depth: i64) -> Vec<RubyValue> {
    let mut out = Vec::new();
    for e in items {
        match e {
            RubyValue::Array(inner) if depth != 0 => {
                let inner = inner.lock().clone();
                out.extend(flatten_to_depth(&inner, depth - 1));
            }
            other => out.push(other.clone()),
        }
    }
    out
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Array.new(size = 0, default = nil)` / `Array.new(size) { |i| ... }`.
    // Reached through `class_method_table` on a `RubyValue::Class` receiver
    // -- `Array` has no generated struct, so there is no constructor for
    // `Class#new`'s ordinary allocator path to find.
    //
    // The default-value form SHARES one object across every slot (real
    // Ruby: `a = Array.new(2, "x"); a[0] << "!"` changes `a[1]` too), which
    // is exactly why the block form exists; a `RubyValue` clone is a handle
    // clone, so that sharing is inherited rather than needing to be built.
    "new" => fn array_new_m(_recv, args, block) {
        arity!(args, 0..=2);
        let size = match args.first() {
            None => 0,
            Some(_) => arg_int!(args, 0),
        };
        if size < 0 {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "negative array size".to_string(),
            ));
        }
        let size = size as usize;
        if let Some(RubyValue::Proc(p)) = &block {
            let mut out = Vec::with_capacity(size);
            for i in 0..size {
                out.push(p.call(&[RubyValue::Int(i as i64)])?);
            }
            return Ok(RubyValue::Array(crate::array_new(out)));
        }
        let fill = args.get(1).cloned().unwrap_or(RubyValue::Nil);
        Ok(RubyValue::Array(crate::array_new(vec![fill; size])))
    }
}


/// `arr[start, len] = value` / `arr[range] = value` -- CRuby's
/// `rb_ary_splice`: replaces the `start..start+len` span with `value`'s
/// `to_ary` coercion (see `splice_elems`), padding with `nil` when `start`
/// lies past the end.
fn array_splice(
    arr: &crate::RArray,
    start: i64,
    len: i64,
    value: &RubyValue,
) -> Result<(), crate::Signal> {
    if len < 0 {
        return Err(crate::dispatch::raise_error(
            "IndexError",
            format!("negative length ({len})"),
        ));
    }
    let elems = splice_elems(value)?;
    let mut items = arr.lock();
    let n = items.len() as i64;
    let start = if start < 0 { start + n } else { start };
    if start < 0 {
        return Err(crate::dispatch::raise_error(
            "IndexError",
            format!("index {} too small for array; minimum: -{n}", start - n),
        ));
    }
    let start = start as usize;
    if start > items.len() {
        items.resize(start, RubyValue::Nil);
    }
    let end = (start + len as usize).min(items.len());
    items.splice(start..end, elems);
    Ok(())
}

/// The splice VALUE's element coercion (CRuby `rb_ary_to_ary`): a plain
/// Array's own elements; a `to_ary`-defining object's coercion (TypeError
/// if it answers a non-Array, non-nil value); anything else as ONE element.
fn splice_elems(value: &RubyValue) -> Result<Vec<RubyValue>, crate::Signal> {
    if let RubyValue::Array(a) = value {
        return Ok(a.lock().clone());
    }
    let to_ary = crate::Symbol::intern("to_ary");
    if crate::dispatch::responds_to(value.class_id(), to_ary, false) {
        match crate::dispatch::send_value(value, to_ary, &[], None)? {
            RubyValue::Array(a) => return Ok(a.lock().clone()),
            RubyValue::Nil => {}
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "can't convert {} to Array ({}#to_ary gives {})",
                        crate::builtins::class_name_of(value),
                        crate::builtins::class_name_of(value),
                        crate::builtins::class_name_of(&other)
                    ),
                ))
            }
        }
    }
    Ok(vec![value.clone()])
}

/// `index_only` -- `dig`'s per-level `[]` (Int index).
fn index_only(recv: &RubyValue, key: &RubyValue) -> Result<RubyValue, crate::Signal> {
    match (recv, key) {
        (RubyValue::Array(a), RubyValue::Int(i)) => Ok(crate::array_get(a, *i)),
        _ => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::class_name_of(key)
            ),
        )),
    }
}

/// `sort`/`sort!`'s comparator: the block when given, `rb_cmp` otherwise
/// (incomparable pairs raise real Ruby's ArgumentError).
pub(crate) fn sort_items(
    items: &mut [RubyValue],
    block: &Option<RubyValue>,
) -> Result<(), crate::Signal> {
    let mut failure: Option<crate::Signal> = None;
    if let Some(RubyValue::Proc(p)) = block {
        items.sort_by(|a, b| {
            if failure.is_some() {
                return std::cmp::Ordering::Equal;
            }
            match p.call(&[a.clone(), b.clone()]) {
                Ok(RubyValue::Int(c)) => c.cmp(&0),
                Ok(_) => {
                    failure = Some(crate::dispatch::raise_error(
                        "ArgumentError",
                        format!(
                            "comparison of {} with {} failed",
                            crate::builtins::class_name_of(a),
                            crate::builtins::class_name_of(b)
                        ),
                    ));
                    std::cmp::Ordering::Equal
                }
                Err(e) => {
                    failure = Some(e);
                    std::cmp::Ordering::Equal
                }
            }
        });
    } else {
        items.sort_by(|a, b| {
            if failure.is_some() {
                return std::cmp::Ordering::Equal;
            }
            match a.rb_cmp(b) {
                Some(c) => c.cmp(&0),
                None => {
                    failure = Some(crate::dispatch::raise_error(
                        "ArgumentError",
                        format!(
                            "comparison of {} with {} failed",
                            crate::builtins::class_name_of(a),
                            crate::builtins::class_name_of(b)
                        ),
                    ));
                    std::cmp::Ordering::Equal
                }
            }
        });
    }
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// `select!`/`reject!`/`keep_if`/`delete_if`'s core: keeps elements whose
/// block result matches `keep`; answers self when anything changed, nil
/// otherwise.
fn in_place_filter(
    recv: &RubyValue,
    meth: &str,
    block: Option<RubyValue>,
    keep: bool,
) -> Result<RubyValue, crate::Signal> {
    let RubyValue::Array(handle) = recv else {
        unreachable!("Array table row dispatched on a non-Array receiver");
    };
    let p = block_or_enum!(recv, meth, &[], block);
    let items = handle.lock().clone();
    let mut out = Vec::with_capacity(items.len());
    for e in items.iter() {
        if p.call(std::slice::from_ref(e))?.truthy() == keep {
            out.push(e.clone());
        }
    }
    let changed = out.len() != items.len();
    *handle.lock() = out;
    Ok(if changed { recv.clone() } else { RubyValue::Nil })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arr(vals: Vec<RubyValue>) -> RubyValue {
        RubyValue::Array(crate::array_new(vals))
    }

    #[test]
    fn push_is_variadic_and_returns_the_receiver() {
        let a = arr(vec![RubyValue::Int(1)]);
        push(&a, &[RubyValue::Int(2), RubyValue::Int(3)], None).unwrap();
        let RubyValue::Array(inner) = &a else { panic!() };
        assert_eq!(inner.lock().len(), 3);
    }

    fn items_of(v: &RubyValue) -> Vec<String> {
        let RubyValue::Array(inner) = v else { panic!("expected an Array") };
        let out = inner.lock().iter().map(|e| e.inspect_string()).collect();
        out
    }

    /// `a[start, len] = v` replaces the SPAN with `v`'s elements -- not the
    /// one-index store `a[i] = v`. Oracle-verified shapes; the
    /// `array_splice` example covers them end to end.
    #[test]
    fn index_set_splices_a_start_length_span() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3), RubyValue::Int(4)]);
        let repl = arr(vec![RubyValue::Int(8), RubyValue::Int(9)]);
        index_set(&a, &[RubyValue::Int(1), RubyValue::Int(2), repl], None).unwrap();
        assert_eq!(items_of(&a), ["1", "8", "9", "4"]);
    }

    #[test]
    fn index_set_with_a_zero_length_inserts_without_removing() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        let repl = arr(vec![RubyValue::Int(9)]);
        index_set(&a, &[RubyValue::Int(1), RubyValue::Int(0), repl], None).unwrap();
        assert_eq!(items_of(&a), ["1", "9", "2"]);
    }

    #[test]
    fn index_set_splice_shrinks_when_the_replacement_is_shorter() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3)]);
        let repl = arr(vec![RubyValue::Int(9)]);
        index_set(&a, &[RubyValue::Int(0), RubyValue::Int(3), repl], None).unwrap();
        assert_eq!(items_of(&a), ["9"]);
    }

    /// A non-Array value inserts as ONE element (no to_ary here).
    #[test]
    fn index_set_splice_inserts_a_scalar_as_one_element() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3)]);
        index_set(&a, &[RubyValue::Int(0), RubyValue::Int(2), RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["9", "3"]);
    }

    #[test]
    fn index_set_splice_past_the_end_nil_pads_first() {
        let a = arr(vec![RubyValue::Int(1)]);
        let repl = arr(vec![RubyValue::Int(9)]);
        index_set(&a, &[RubyValue::Int(3), RubyValue::Int(0), repl], None).unwrap();
        assert_eq!(items_of(&a), ["1", "nil", "nil", "9"]);
    }

    /// The expression VALUE is the right-hand side as written -- never the
    /// coercion, never the receiver.
    #[test]
    fn index_set_splice_answers_the_value_as_written() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        let repl = arr(vec![RubyValue::Int(9)]);
        let out = index_set(&a, &[RubyValue::Int(0), RubyValue::Int(1), repl], None).unwrap();
        assert_eq!(out.inspect_string(), "[9]");
    }

    #[test]
    fn index_set_splices_an_inclusive_range() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3), RubyValue::Int(4)]);
        let range = RubyValue::Range(
            Some(Box::new(RubyValue::Int(1))),
            Some(Box::new(RubyValue::Int(2))),
            false,
        );
        index_set(&a, &[range, RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["1", "9", "4"]);
    }

    #[test]
    fn index_set_splices_an_exclusive_range() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3), RubyValue::Int(4)]);
        let range = RubyValue::Range(
            Some(Box::new(RubyValue::Int(1))),
            Some(Box::new(RubyValue::Int(3))),
            true,
        );
        index_set(&a, &[range, RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["1", "9", "4"]);
    }

    /// An endless range splices to the end; a beginless one from the start.
    #[test]
    fn index_set_splices_open_ended_ranges() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3)]);
        let endless = RubyValue::Range(Some(Box::new(RubyValue::Int(1))), None, false);
        index_set(&a, &[endless, RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["1", "9"]);

        let b = arr(vec![RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3)]);
        let beginless = RubyValue::Range(None, Some(Box::new(RubyValue::Int(1))), false);
        index_set(&b, &[beginless, RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&b), ["9", "3"]);
    }

    /// The plain one-index store still stores (no splice).
    #[test]
    fn index_set_with_two_args_and_an_int_index_stores_one_element() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        index_set(&a, &[RubyValue::Int(0), RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["9", "2"]);
    }

    #[test]
    fn to_h_maps_each_element_through_a_block() {
        let a = arr(vec![
            RubyValue::Array(crate::array_new(vec![RubyValue::Int(1), RubyValue::Int(2)])),
        ]);
        // `{ |pair| [pair[1], pair[0]] }` -- an Array yields ONE value per
        // element, so the block sees the element itself.
        let p: crate::RProc = crate::RProc::new(|args: &[RubyValue]| {
            let RubyValue::Array(pair) = &args[0] else { panic!("expected the element") };
            let pair = pair.lock().clone();
            Ok(RubyValue::Array(crate::array_new(vec![pair[1].clone(), pair[0].clone()])))
        });
        let out = to_h(&a, &[], Some(RubyValue::Proc(p))).unwrap();
        assert_eq!(out.inspect_string(), "{2 => 1}");
    }

    #[test]
    fn to_h_without_a_block_requires_pair_shaped_elements() {
        let a = arr(vec![
            RubyValue::Array(crate::array_new(vec![RubyValue::Int(1), RubyValue::Int(2)])),
        ]);
        let out = to_h(&a, &[], None).unwrap();
        assert_eq!(out.inspect_string(), "{1 => 2}");
    }

    #[test]
    fn index_rejects_a_symbol_with_a_type_error_shape() {
        let a = arr(vec![RubyValue::Int(1)]);
        let sym = RubyValue::Symbol(crate::Symbol::intern("x"));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| index(&a, &[sym], None)));
        assert!(r.is_err()); // registry-less: TypeError surfaces as a panic
    }

    #[test]
    fn each_snapshots_and_returns_the_receiver() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        let seen = std::sync::Arc::new(parking_lot::Mutex::new(0i64));
        let seen2 = std::sync::Arc::clone(&seen);
        let p: crate::RProc = crate::RProc::new(move |args: &[RubyValue]| {
            if let RubyValue::Int(i) = &args[0] {
                *seen2.lock() += i;
            }
            Ok(RubyValue::Nil)
        });
        each(&a, &[], Some(RubyValue::Proc(p))).unwrap();
        assert_eq!(*seen.lock(), 3);
    }
}
