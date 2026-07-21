# A value-form && / || whose unified type is nil/void, and a case whose
# scrutinee is an empty container literal, must not declare a `void` C temp
# (an empty `()` operand or `[]` scrutinee has no scalar storage type).
p((() && true))
p((true && ()))
p((() and true))
p((true or ()))
r = case []
    when [] then "empty"
    else "other"
    end
p r
h = case {}
    when {} then "eh"
    else "oh"
    end
p h
