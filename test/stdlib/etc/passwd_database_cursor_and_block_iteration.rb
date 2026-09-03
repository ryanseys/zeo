require "etc"
# The block form iterates the whole database; every row is an Etc::Passwd
# and root (uid 0) is always present.
count = 0
saw_root = false
Etc.passwd do |p|
  count += 1
  saw_root = true if p.uid == 0
end
puts count > 0
puts saw_root
# setpwent rewinds; getpwent then returns a row (or nil at the end).
Etc.setpwent
first = Etc.getpwent
puts(first.nil? || first.is_a?(Etc::Passwd))
Etc.endpwent
__END__
true
true
true
