class Server
  def self.run(sfd)
    f = Fiber.new { sfd + 1 }
    f.resume
  end
  def self.work(sfd)
    sfd * 2
  end
end

class Worker
  def run(sfd)
    f = Fiber.new { sfd + 1 }
    f.resume
  end
  def work(sfd)
    sfd * 2
  end
end

puts Server.run(10)
puts Server.work(5)
w = Worker.new
puts w.run(10)
puts w.work(5)
