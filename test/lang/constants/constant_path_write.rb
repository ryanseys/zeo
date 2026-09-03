# `Mod::X = v` assigns through a constant path. Spinel requires the constant to
# be declared first (registered by analysis); reassigning it then works, as the
# `M::X ||= v` / `M::X += v` forms already did. (Output is compared on stdout;
# Ruby's "already initialized constant" warning goes to stderr.)

module M
  COUNT = 0
  NAME = ""
end

M::COUNT = 5
p M::COUNT                  # 5

M::NAME = "spinel"
p M::NAME.upcase            # "SPINEL"

# reassign using the constant's own value
M::COUNT = M::COUNT + 10
p M::COUNT                  # 15

# nested constant path
module A
  module B
    LEVEL = 1
  end
end
A::B::LEVEL = 42
p A::B::LEVEL              # 42

# the operator / or-write forms still work alongside plain write
module N
  V = 1
end
N::V = 3
N::V += 4
p N::V                     # 7
N::V ||= 99
p N::V                     # 7 (already truthy)

# an empty [] / {} reassignment keeps the constant's specialized array/hash type
module E
  NUMS = [1, 2, 3]
  TBL = { "a" => 1 }
end
E::NUMS = []
p E::NUMS                  # []
E::TBL = {}
p E::TBL                   # {}
__END__
5
"SPINEL"
15
42
7
7
[]
{}
#@ stderr
lang/constants/constant_path_write.rb:11: warning: already initialized constant M::COUNT
lang/constants/constant_path_write.rb:7: warning: previous definition of COUNT was here
lang/constants/constant_path_write.rb:14: warning: already initialized constant M::NAME
lang/constants/constant_path_write.rb:8: warning: previous definition of NAME was here
lang/constants/constant_path_write.rb:18: warning: already initialized constant M::COUNT
lang/constants/constant_path_write.rb:11: warning: previous definition of COUNT was here
lang/constants/constant_path_write.rb:27: warning: already initialized constant A::B::LEVEL
lang/constants/constant_path_write.rb:24: warning: previous definition of LEVEL was here
lang/constants/constant_path_write.rb:34: warning: already initialized constant N::V
lang/constants/constant_path_write.rb:32: warning: previous definition of V was here
lang/constants/constant_path_write.rb:35: warning: already initialized constant N::V
lang/constants/constant_path_write.rb:34: warning: previous definition of V was here
lang/constants/constant_path_write.rb:45: warning: already initialized constant E::NUMS
lang/constants/constant_path_write.rb:42: warning: previous definition of NUMS was here
lang/constants/constant_path_write.rb:47: warning: already initialized constant E::TBL
lang/constants/constant_path_write.rb:43: warning: previous definition of TBL was here
