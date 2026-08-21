# A constant bound to an EMPTY array literal has no element type to read off the
# literal. That left it typed UNKNOWN, which downstream does not read as
# "unknown" but as UNDEFINED: every reference reported the constant as defined
# nowhere and raised NameError, where CRuby answers the array. Filling it through
# `CONST[i] = v` -- the table-building shape -- is now element evidence like
# `push`, and a constant with no evidence at all still gets the default an empty
# literal bound to a local gets.
EMPTY = []
def read_empty; EMPTY; end
p read_empty

TABLE = []
TABLE[0] = :zero
TABLE[2] = :two
def read_table; TABLE; end
p read_table

class Codes
  BY_ID = []
  def self.record(id, name)
    BY_ID[id] = name
  end
end
Codes.record(1, "one")
Codes.record(3, "three")
p Codes::BY_ID

# Element evidence still wins over the default: pushed ints stay an int array.
NUMS = []
NUMS.push(1)
NUMS.push(2)
def read_nums; NUMS; end
p read_nums
p read_nums.sum
