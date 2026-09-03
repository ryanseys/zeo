# A blank instance is a legal RECEIVER, and answers exactly what ruby's does.
#
# Making `allocate` work is only half the job. Ruby draws a line between an
# uninitialized value and an empty one, and the two are not interchangeable:
# `Regexp.allocate` refuses `#source` where `//` answers `""`, and
# `StringIO.allocate` refuses `#read` where `StringIO.new("")` answers `""`.
# A blank that quietly WORKED would hand back an answer ruby never gives --
# `//` matching everything is the worst of them.
#
# Two classes needed a new marker to tell the states apart, because no
# existing field could: `IoBackend::Uninit` (an unopened stream is not a
# closed one -- different message, and ruby names it by address rather than by
# a path it has not got) and `QueueInner::initialized`. The Queue one is not
# cosmetic: `SizedQueue.allocate`'s bound is 0, so an unguarded `push`
# back-pressures on room that can never arrive and the program DEADLOCKS where
# ruby raises.
#
# Every other class already had the state -- `Fiber::uninitialized`,
# `RDir::open`, zlib's `closed` -- and only needed the blank to select it.

require "stringio"
require "strscan"
require "zlib"
require "date"

def row(label)
  puts "#{label}\t#{(yield).inspect}"
rescue ScriptError, StandardError => e
  puts "#{label}\t!#{e.class}: #{e.message}"
end

row("Regexp#source") { Regexp.allocate.source }
row("Regexp#casefold?") { Regexp.allocate.casefold? }
row("Regexp#to_s") { Regexp.allocate.to_s }
# `#encoding` and `#==` are the two rows a blank Regexp DOES answer, and the
# encoding is the binary one: there is no source to compute one from.
row("Regexp#encoding") { Regexp.allocate.encoding }
row("Regexp#==") { r = Regexp.allocate; r == r }

row("Enumerator#each") { Enumerator.allocate.each { |x| x } }
row("Enumerator#size") { Enumerator.allocate.size }
row("Enumerator#inspect") { Enumerator.allocate.inspect }
row("Lazy#inspect") { Enumerator::Lazy.allocate.inspect }
row("Lazy#first") { Enumerator::Lazy.allocate.first }

row("StringIO#read") { StringIO.allocate.read }
row("StringIO#closed?") { StringIO.allocate.closed? }
# `reopen` is the row that SEEDS a blank, so it is not a dead end.
row("StringIO#reopen") { io = StringIO.allocate; io.reopen("hi"); io.read }

row("StringScanner#scan") { StringScanner.allocate.scan(/a/) }
row("StringScanner#inspect") { StringScanner.allocate.inspect }

row("IO#fileno") { IO.allocate.fileno }
row("IO#closed?") { IO.allocate.closed? }
row("IO#path") { IO.allocate.path }
row("File#size") { File.allocate.size }
row("Dir#read") { Dir.allocate.read }
row("Dir#path") { Dir.allocate.path }

row("Queue#push") { Queue.allocate.push(1) }
row("Queue#size") { Queue.allocate.size }
row("Queue#closed?") { Queue.allocate.closed? }
# The seeding row again: `initialize` makes a blank usable.
row("Queue#initialize") { q = Queue.allocate; q.send(:initialize); q.push(1); q.pop }
row("SizedQueue#push") { SizedQueue.allocate.push(1) }
row("SizedQueue#max") { SizedQueue.allocate.max }

row("Zlib::Deflate#deflate") { Zlib::Deflate.allocate.deflate("x") }
row("Zlib::Deflate#closed?") { Zlib::Deflate.allocate.closed? }
row("Fiber#resume") { Fiber.allocate.resume }

# The blanks ruby lets you USE, which need no marker at all: an empty one IS
# the blank, and every row reads it.
row("Set#add") { s = Set.allocate; s.add(1); s.to_a }
row("Mutex#lock") { m = Mutex.allocate; m.lock; m.unlock; "locked" }
row("ThreadGroup#list") { ThreadGroup.allocate.list }
row("WeakMap#size") { ObjectSpace::WeakMap.allocate.size }
row("Date#to_s") { Date.allocate.to_s }
row("Date#jd") { Date.allocate.jd }
row("Process::Status#to_i") { Process::Status.allocate.to_i }
# ...but `Process::Status` still names the state in `#inspect`, because pid 0
# with a status word of 0 is a legal REAL status that `#to_s` prints the same.
row("Process::Status#inspect") { Process::Status.allocate.inspect }
row("Process::Status#to_s") { Process::Status.allocate.to_s }
__END__
Regexp#source	!TypeError: uninitialized Regexp
Regexp#casefold?	!TypeError: uninitialized Regexp
Regexp#to_s	!TypeError: uninitialized Regexp
Regexp#encoding	#<Encoding:BINARY (ASCII-8BIT)>
Regexp#==	true
Enumerator#each	!ArgumentError: uninitialized enumerator
Enumerator#size	!ArgumentError: uninitialized enumerator
Enumerator#inspect	"#<Enumerator: uninitialized>"
Lazy#inspect	"#<Enumerator::Lazy: uninitialized>"
Lazy#first	!ArgumentError: uninitialized enumerator
StringIO#read	!IOError: uninitialized stream
StringIO#closed?	!IOError: uninitialized stream
StringIO#reopen	!IOError: uninitialized stream
StringScanner#scan	!ArgumentError: uninitialized StringScanner object
StringScanner#inspect	"#<StringScanner (uninitialized)>"
IO#fileno	!IOError: uninitialized stream
IO#closed?	!IOError: uninitialized stream
IO#path	nil
File#size	!IOError: uninitialized stream
Dir#read	!IOError: closed directory
Dir#path	nil
Queue#push	!TypeError: #<Thread::Queue:0xADDR> not initialized
Queue#size	!TypeError: #<Thread::Queue:0xADDR> not initialized
Queue#closed?	false
Queue#initialize	1
SizedQueue#push	!TypeError: #<Thread::SizedQueue:0xADDR> not initialized
SizedQueue#max	0
Zlib::Deflate#deflate	!Zlib::Error: stream is not ready
Zlib::Deflate#closed?	true
Fiber#resume	!FiberError: uninitialized fiber
Set#add	[1]
Mutex#lock	"locked"
ThreadGroup#list	[]
WeakMap#size	0
Date#to_s	"-4712-01-01"
Date#jd	0
Process::Status#to_i	0
Process::Status#inspect	"#<Process::Status: uninitialized>"
Process::Status#to_s	"pid 0 exit 0"
