# A global that holds different types across assignments has a poly (sp_RbVal)
# slot; assigning a bare scalar (`$g = 42`) into it must box the value, not
# store the raw int into the sp_RbVal slot (a C compile error).
$g = 5
$g = "hello"
$g = [1, 2]
p $g
$g = 42
p $g
$g = :sym
p $g
$g = true
p $g
