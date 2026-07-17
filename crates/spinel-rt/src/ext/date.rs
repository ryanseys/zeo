//! `date` (CRuby's bundled `date` gem). **Scaffolded** -- `require "date"`
//! activates the `Date`/`DateTime` constants so downstream code compiles past
//! the require, but the calendar arithmetic is not yet implemented (`todo!()`
//! markers). A real implementation would carry an astronomical/Julian-day core
//! (like CRuby's `date_core.c`); see docs/EXTENSIONS.md.
//!
//! Both `Date` and `DateTime` dispatch here.

use crate::builtins::builtin_methods;

builtin_methods! {
    pub(crate) fn lookup;

    "year" => fn year(_recv, _args, _block) { todo!("Date#year -- see docs/EXTENSIONS.md") }
    "month" | "mon" => fn month(_recv, _args, _block) { todo!("Date#month") }
    "day" | "mday" => fn day(_recv, _args, _block) { todo!("Date#day") }
    "wday" => fn wday(_recv, _args, _block) { todo!("Date#wday") }
    "to_s" => fn to_s(_recv, _args, _block) { todo!("Date#to_s") }
    "strftime" => fn strftime(_recv, _args, _block) { todo!("Date#strftime") }
    "+" => fn plus(_recv, _args, _block) { todo!("Date#+") }
    "-" => fn minus(_recv, _args, _block) { todo!("Date#-") }
    "<=>" => fn cmp(_recv, _args, _block) { todo!("Date#<=>") }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" | "civil" => fn new_m(_recv, _args, _block) { todo!("Date.new -- see docs/EXTENSIONS.md") }
    "today" => fn today(_recv, _args, _block) { todo!("Date.today") }
    "parse" => fn parse(_recv, _args, _block) { todo!("Date.parse") }
    "jd" => fn jd(_recv, _args, _block) { todo!("Date.jd") }
}
