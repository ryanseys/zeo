#ifndef RBIMPL_RFILE_H                               /*-*-C++-*-vi:se ft=cpp:*/
#define RBIMPL_RFILE_H
/**
 * @file
 * @author     Ruby developers <ruby-core@ruby-lang.org>
 * @copyright  This  file  is   a  part  of  the   programming  language  Ruby.
 *             Permission  is hereby  granted,  to  either redistribute  and/or
 *             modify this file, provided that  the conditions mentioned in the
 *             file COPYING are met.  Consult the file for details.
 * @warning    Symbols   prefixed  with   either  `RBIMPL`   or  `rbimpl`   are
 *             implementation details.   Don't take  them as canon.  They could
 *             rapidly appear then vanish.  The name (path) of this header file
 *             is also an  implementation detail.  Do not expect  it to persist
 *             at the place it is now.  Developers are free to move it anywhere
 *             anytime at will.
 * @note       To  ruby-core:  remember  that   this  header  can  be  possibly
 *             recursively included  from extension  libraries written  in C++.
 *             Do not  expect for  instance `__VA_ARGS__` is  always available.
 *             We assume C99  for ruby itself but we don't  assume languages of
 *             extension libraries.  They could be written in C++98.
 * @brief      Defines struct ::RFile.
 */
#include "ruby/internal/core/rbasic.h"
#include "ruby/internal/cast.h"
#include "ruby/internal/zeo.h"

/* rb_io_t is in ruby/io.h.  The header file has historically not been included
 * into ruby/ruby.h.  We follow that tradition. */
struct rb_io;

/**
 * Ruby's File  and IO.  Ruby's  IO are not  just file descriptors.   They have
 * buffers.   They also  have  encodings.  Various  information are  controlled
 * using this struct.
 */
struct RFile {

    /** Basic part, including flags and class. */
    struct RBasic basic;

    /** IO's specific fields. */
    struct rb_io *fptr;
};

/**
 * Convenient casting macro.
 *
 * @param   obj  An object, which is in fact an ::RFile.
 * @return  The passed object casted to ::RFile.
 */
/* zeo: a call, not a cast -- a refilled view. `fptr` names a `struct rb_io`
 * minted beside the object and filled from the IO on every reach, so
 * `GetOpenFile(io, fp); fp->fd` answers the descriptor the IO has right now.
 * The fields zeo cannot honour -- the buffers, the converters -- stay zero,
 * and a store to the view does not reach the IO. See
 * `ruby/internal/zeo.h`. */
#define RFILE(obj) rb_zeo_rfile(RBIMPL_CAST((VALUE)(obj)))
#endif /* RBIMPL_RFILE_H */
