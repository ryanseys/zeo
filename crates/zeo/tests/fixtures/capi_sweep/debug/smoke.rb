require "debug/frame_info"

puts DEBUGGER__.respond_to?(:frame_depth), DEBUGGER__.respond_to?(:capture_frames)
puts ObjectSpace.respond_to?(:count_iseq)
puts RubyVM::InstructionSequence.instance_methods(false).include?(:type)
