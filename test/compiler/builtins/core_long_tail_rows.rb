# The long tail: one row apiece across Array, Class, Dir, Encoding,
#@ only: macos
# Enumerator::Lazy, GC, Hash, Marshal, Process, Random, Regexp, Set, String,
# Symbol, Warning and the Thread family.

require "set"
require "tmpdir"

puts "Array.[]: #{Array[1, 2, 3].inspect}"
puts "Array.[] empty: #{Array[].inspect}"

puts "attached_object: #{String.singleton_class.attached_object}"
begin
  Class.new.attached_object
rescue TypeError => e
  puts "attached_object plain: #{e.message.sub(/0x\h+/, '0xADDR')}"
end

puts "Encoding.aliases: #{Encoding.aliases['BINARY']}"
puts "Encoding.locale_charmap: #{Encoding.locale_charmap.class}"
puts "Encoding._dump: #{Encoding::UTF_8._dump(0)}"
puts "Encoding._load: #{Encoding._load('UTF-8')}"
# CRuby's `_load` answers what it was handed rather than the singleton.
puts "Encoding._load class: #{Encoding._load('UTF-8').class}"

puts "lazy eager: #{[1, 2, 3].lazy.map { |x| x * 2 }.eager.class}"
puts "lazy eager values: #{[1, 2, 3].lazy.map { |x| x * 2 }.eager.to_a.inspect}"

puts "GC#garbage_collect listed: #{GC.instance_methods(false).include?(:garbage_collect)}"

marked = Hash.ruby2_keywords_hash({ a: 1 })
puts "ruby2_keywords_hash: #{marked.inspect}"
puts "ruby2_keywords_hash? plain: #{Hash.ruby2_keywords_hash?({})}"

puts "Marshal.restore: #{Marshal.restore(Marshal.dump([1, 'two', :three])).inspect}"

puts "Random.seed: #{Random.seed.is_a?(Integer)}"

puts "Regexp.timeout default: #{Regexp.timeout.inspect}"
Regexp.timeout = 3
puts "Regexp.timeout set: #{Regexp.timeout.inspect}"
Regexp.timeout = nil
puts "Regexp.timeout cleared: #{Regexp.timeout.inspect}"
$_ = "hello world"
puts "Regexp#~: #{~/world/}"
puts "Regexp#~ miss: #{(~/nope/).inspect}"

s = Set[1, 2]
puts "compare_by_identity? before: #{s.compare_by_identity?}"
puts "compare_by_identity: #{s.compare_by_identity.class}"
puts "compare_by_identity? after: #{s.compare_by_identity?}"

str = +"é"
puts "unicode_normalize!: #{str.unicode_normalize!(:nfc).bytes.inspect}"
puts "unicode_normalize! in place: #{str.bytes.inspect}"

puts "Symbol.all_symbols: #{Symbol.all_symbols.is_a?(Array)}"
puts "Symbol.all_symbols has one: #{Symbol.all_symbols.include?(:all_symbols_probe_sym)}"

puts "Warning.categories: #{Warning.categories.include?(:deprecated)}"
puts "Warning#warn listed: #{Warning.instance_methods(false).inspect}"

puts "Process::Tms.members: #{Process::Tms.members.inspect}"
puts "Process::Tms.keyword_init?: #{Process::Tms.keyword_init?.inspect}"
tms = Process::Tms[1, 2, 3, 4]
puts "Process::Tms.[]: #{tms.inspect}"
tms.utime = 9.5
puts "Process::Tms writer: #{tms.utime}"
puts "Process::Tms to_a: #{tms.to_a.inspect}"

pid = Process.spawn("true")
st = Process::Status.wait(pid)
puts "Status.wait: #{st.class}"
puts "Status.wait exit: #{st.exitstatus}"
puts "Status coredump?: #{st.coredump?}"
puts "Status stopsig: #{st.stopsig.inspect}"

q = Thread::Queue.new
q << 1
q << 2
puts "Queue num_waiting: #{q.num_waiting}"
puts "Queue clear: #{q.clear.class}"
puts "Queue size after clear: #{q.size}"
begin
  q.marshal_dump
rescue TypeError => e
  puts "Queue marshal_dump: #{e.message}"
end
begin
  Thread::ConditionVariable.new.marshal_dump
rescue TypeError => e
  puts "CV marshal_dump: #{e.message}"
end

sq = Thread::SizedQueue.new(2)
puts "SizedQueue num_waiting: #{sq.num_waiting}"
sq << 1
puts "SizedQueue clear: #{sq.clear.class}"

m = Thread::Mutex.new
m.lock
puts "Mutex#sleep: #{m.sleep(0.001).inspect}"
puts "Mutex still locked: #{m.locked?}"
m.unlock

g = ThreadGroup.new
puts "ThreadGroup enclosed? before: #{g.enclosed?}"
puts "ThreadGroup enclose: #{g.enclose.class}"
puts "ThreadGroup enclosed? after: #{g.enclosed?}"
puts "Default enclosed?: #{ThreadGroup::Default.enclosed?}"

puts "top-level Queue: #{Queue.equal?(Thread::Queue)}"
puts "top-level Mutex: #{Mutex.equal?(Thread::Mutex)}"
puts "top-level SizedQueue: #{SizedQueue.equal?(Thread::SizedQueue)}"
puts "top-level ConditionVariable: #{ConditionVariable.equal?(Thread::ConditionVariable)}"
puts "RUBY_COPYRIGHT: #{RUBY_COPYRIGHT.start_with?('ruby - Copyright')}"
puts "File::Separator: #{File::Separator.inspect}"

Dir.mktmpdir do |dir|
  Dir.open(dir) do |d|
    inside = d.chdir { File.basename(Dir.pwd) }
    puts "Dir#chdir block: #{inside == File.basename(dir)}"
  end
end
begin
  Dir.chroot("/nonexistent-zeo-probe")
rescue SystemCallError => e
  puts "Dir.chroot: #{e.class}"
end
__END__
Array.[]: [1, 2, 3]
Array.[] empty: []
attached_object: String
attached_object plain: '#<Class:0xADDR>' is not a singleton class
Encoding.aliases: ASCII-8BIT
Encoding.locale_charmap: String
Encoding._dump: UTF-8
Encoding._load: UTF-8
Encoding._load class: String
lazy eager: Enumerator
lazy eager values: [2, 4, 6]
GC#garbage_collect listed: true
ruby2_keywords_hash: {a: 1}
ruby2_keywords_hash? plain: false
Marshal.restore: [1, "two", :three]
Random.seed: true
Regexp.timeout default: nil
Regexp.timeout set: 3.0
Regexp.timeout cleared: nil
Regexp#~: 6
Regexp#~ miss: nil
compare_by_identity? before: false
compare_by_identity: Set
compare_by_identity? after: true
unicode_normalize!: [195, 169]
unicode_normalize! in place: [195, 169]
Symbol.all_symbols: true
Symbol.all_symbols has one: true
Warning.categories: true
Warning#warn listed: [:warn]
Process::Tms.members: [:utime, :stime, :cutime, :cstime]
Process::Tms.keyword_init?: nil
Process::Tms.[]: #<struct Process::Tms utime=1, stime=2, cutime=3, cstime=4>
Process::Tms writer: 9.5
Process::Tms to_a: [9.5, 2, 3, 4]
Status.wait: Process::Status
Status.wait exit: 0
Status coredump?: false
Status stopsig: nil
Queue num_waiting: 0
Queue clear: Thread::Queue
Queue size after clear: 0
Queue marshal_dump: can't dump Thread::Queue
CV marshal_dump: can't dump Thread::ConditionVariable
SizedQueue num_waiting: 0
SizedQueue clear: Thread::SizedQueue
Mutex#sleep: nil
Mutex still locked: true
ThreadGroup enclosed? before: false
ThreadGroup enclose: ThreadGroup
ThreadGroup enclosed? after: true
Default enclosed?: false
top-level Queue: true
top-level Mutex: true
top-level SizedQueue: true
top-level ConditionVariable: true
RUBY_COPYRIGHT: true
File::Separator: "/"
Dir#chdir block: true
Dir.chroot: Errno::EPERM
