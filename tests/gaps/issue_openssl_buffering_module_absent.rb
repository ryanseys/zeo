# `OpenSSL::Buffering` does not exist as a module. CRuby mixes it into
# SSLSocket to supply the buffered IO surface (read/gets/puts/readpartial/
# each_line/...); zeo implements those methods directly on SSLSocket
# instead, because a native object carries no ivar table for the module's
# @rbuffer/@sync state. Every method works -- what fails is naming the
# module, and any code that reopens it or checks the ancestry.
#
# Closing this needs ivar-bearing native objects (or an ivar-backed
# SSLSocket), after which the gem's Ruby half can vendor upstream's
# buffering.rb verbatim.
require "openssl"

p OpenSSL::SSL::SSLSocket.include?(OpenSSL::Buffering)
p OpenSSL::Buffering::BLOCK_SIZE
p OpenSSL::Buffering.instance_methods(false).include?(:readpartial)
