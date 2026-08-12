# A C struct whose shape depends on the platform is written as a list the
# class body builds up and then splats into one `layout` -- sys-uname's
# `utsname` is the canonical case, and the same idiom decides the field list
# of a good part of the FFI corpus.
#
# Every step of the build is a compile-time fact: the array literal, the
# guarded `push`es, the `<<`/`concat` appends, and the `case` that sizes the
# char arrays. zeo folds the guards and replays the array, so the struct
# lowers to the same fixed layout the C header describes. A build step zeo
# could NOT follow stays a rejection -- a short struct would be a wrong one.
require "ffi"

class Utsname < FFI::Struct
  # The buffer width is spelled as a `case` on the older platform question.
  # Both live branches answer 16 here so the program's output does not depend
  # on which one this build picks.
  case RbConfig::CONFIG["host_os"]
  when /darwin/i
    BUFSIZE = 16
  when /linux/i
    BUFSIZE = 16
  else
    BUFSIZE = 8
  end

  members = [
    :sysname,  [:char, BUFSIZE],
    :nodename, [:char, BUFSIZE]
  ]

  members.push(:release, [:char, BUFSIZE]) if RbConfig::CONFIG["host_os"] =~ /darwin|linux/i
  members.push(:__id_number, [:char, BUFSIZE]) if RbConfig::CONFIG["host_os"] =~ /hpux/i
  members << :flags
  members << :int
  members.concat([:count, :uint32])

  layout(*members)
end

p Utsname::BUFSIZE
p Utsname.size
p Utsname.offset_of(:sysname)
p Utsname.offset_of(:release)
p Utsname.offset_of(:flags)
p Utsname.offset_of(:count)
p Utsname.members

s = Utsname.new
s[:flags] = 7
s[:count] = 9
p s[:flags]
p s[:count]
puts "still running"
