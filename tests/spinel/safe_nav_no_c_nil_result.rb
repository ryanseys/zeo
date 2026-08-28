# A `&.` whose answer has no C nil to hold one -- a Class, a Symbol, a Rational,
# a Complex -- has to be poly, the way a C bool already had to be. The rules
# that said so were placed by position in the inference chain, so the names
# resolved earlier (`class` at the object arm, `to_sym` at the String arm) kept
# their concrete type while the emitted guard boxed nil into it anyway.

def klass(v) = v&.class
def as_sym(v) = v&.to_sym
def ident(v) = v&.object_id

["abc", nil].each do |v|
  p klass(v)
  p as_sym(v)
  p ident(v).nil?
end

# the types that DO have a C nil keep it, and keep their concrete type: the
# guard's nil arm is the array's NULL, the string's NULL, the int's sentinel
def letters(v) = v&.chars
def up(v) = v&.upcase
def len(v) = v&.size

["abc", nil].each do |v|
  p letters(v)
  p up(v)
  p len(v)
end

# a Class answer flowing on: the widened value still reads as a Class
p klass("x") == String
p klass(nil) == String
p klass(nil).nil?
p as_sym("x") == :x
p as_sym(nil).nil?
