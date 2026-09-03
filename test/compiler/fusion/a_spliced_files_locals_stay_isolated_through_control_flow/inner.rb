# The required file: same local names, different values, each assigned from
# inside the same control-flow shapes.
name = "inner"
count = 100

if name == "inner"
  count = 200
end

while count < 202
  count += 1
end

case count
when 202 then tag = "two-oh-two"
else tag = "other"
end

begin
  parts = ["x"]
rescue StandardError
  parts = []
end

interp = "#{name}-#{count}-#{tag}"
listed = [name, count, tag].map { |v| v.to_s }

def inner_report
  :inner_ran
end

p [name, count, tag, parts, interp, listed]
