# coverage is a CRuby C extension (line/branch coverage instrumentation
# hooking the VM) that zeo has no native implementation of -- `require
# "coverage"` raises LoadError.
require "coverage"
p Coverage.respond_to?(:start)
