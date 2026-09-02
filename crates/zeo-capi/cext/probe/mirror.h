/*
 * The surface bindgen reads to write `$OUT_DIR/cext_layout.rs`.
 *
 * Every struct zeo's views fill has its layout defined in the vendored
 * headers, and the Rust that fills one must agree with it to the byte. A
 * hand-written `#[repr(C)]` copy would be a SECOND OWNER of that fact, and a
 * wrong offset there reads or writes an unrelated field -- so the Rust is
 * generated from these headers instead, with bindgen's size and offset
 * assertions on.
 *
 * `ruby/re.h` is here for `struct re_registers`: `rb_matchext_t` embeds one
 * and `rmatch.h` only forward-declares it. It has to follow `ruby/ruby.h`,
 * which is what gives `ruby/onigmo.h` the fixed-width integer types it uses
 * without including `<stdint.h>` itself.
 */

#include "ruby/ruby.h"
#include "ruby/re.h"
#include "ruby/io.h"
#include "ruby/encoding.h"
