# `when *array` is membership; `when <Rational>` / `when <Complex>` are
# value equality. All three previously failed (splat never matched;
# Rational/Complex produced a C compile error from `int == struct`).
a = [1, 2, 3]
case 2
when *a then puts "in"
else puts "out"
end
case 9
when *a then puts "in"
else puts "out"
end

case 0
when 0r then puts "r-zero"
else puts "r-other"
end
case 1
when 0r then puts "r-zero"
else puts "r-other"
end

case 0
when 0i then puts "c-zero"
else puts "c-other"
end
case 1
when 0i then puts "c-zero"
else puts "c-other"
end
