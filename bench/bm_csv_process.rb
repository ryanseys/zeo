# CSV processor - process only (no massive string concat in generate)

def generate_lines(n)
  depts = ["eng", "sales", "hr", "ops"]
  names = ["alice", "bob", "carol", "dave", "eve"]
  lines = ["name,dept,salary,age"]
  i = 0
  while i < n
    name = names[i % 5]
    dept = depts[i % 4]
    salary = 30000 + (i * 1337) % 70000
    age = 22 + i % 40
    lines.push(name + "," + dept + "," + salary.to_s + "," + age.to_s)
    i = i + 1
  end
  lines
end

def process_lines(lines)
  total_salary = 0
  high_earners = 0
  dept_counts = {}
  i = 1
  while i < lines.length
    fields = lines[i].split(",")
    if fields.length >= 4
      dept = fields[1]
      salary = fields[2].to_i
      total_salary = total_salary + salary
      if salary >= 80000
        high_earners = high_earners + 1
      end
      if dept_counts.has_key?(dept)
        dept_counts[dept] = dept_counts[dept] + 1
      else
        dept_counts[dept] = 1
      end
    end
    i = i + 1
  end
  puts lines.length - 1
  puts total_salary
  puts high_earners
  dept_counts.each { |k, v|
    puts k + ": " + v.to_s
  }
end

lines = generate_lines(5000)
process_lines(lines)

# Benchmark: process 200 times
i = 0
while i < 200
  process_lines(lines)
  i = i + 1
end
puts "done"
