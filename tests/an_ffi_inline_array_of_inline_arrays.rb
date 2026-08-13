# `layout :matrix, [[:XFixed, 3], 3]` -- X11's XTransform, a 3x3 matrix. C
# lays a 2-D array out as rows of rows, so the field is 9 elements wide and
# the next field starts after all of it.
#
# The gem computes that layout and then REFUSES to read one back: indexing the
# proxy raises `ArgumentError: get not supported for FFI::ArrayType`, because
# an array has no scalar accessor to index with. `size` and `to_ptr` still
# answer, and a whole-field write is the same NotImplementedError any inline
# array gives.
require "ffi"

class Matrix3 < FFI::Struct
  layout :matrix, [[:int32, 3], 3],
         :tail, :int32
end

class Deep < FFI::Struct
  layout :cube, [[[:int8, 2], 3], 4],
         :tail, :int32
end

def probe(label)
  puts "#{label}: #{yield.inspect}"
rescue StandardError => e
  puts "#{label}: #{e.class}: #{e.message}"
end

p [Matrix3.size, Matrix3.alignment, Matrix3.members, Matrix3.offset_of(:tail)]
p [Deep.size, Deep.alignment, Deep.offset_of(:tail)]

m = Matrix3.new
row = m[:matrix]
p row.class
probe("size") { row.size }
probe("get") { row[0] }
probe("set") { row[0] = 1 }
probe("to_a") { row.to_a }
probe("each") { row.each { |x| x } }
probe("to_ptr offset") { row.to_ptr.address - m.to_ptr.address }

begin
  m[:matrix] = [1, 2]
rescue NotImplementedError => e
  puts "whole write: #{e.class}: #{e.message}"
end

# A field AFTER the matrix still reads and writes normally -- the point of
# getting the width right.
m[:tail] = 42
p m[:tail]
