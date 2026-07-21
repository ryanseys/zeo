class Combat
  def initialize
    @count = 3
    @name = "shotgun"
  end

  def sprites
    @count * 2
  end

  def update(n)
    @count += n
    @count
  end

  def name
    @name
  end
end

class Player
  def initialize(armed)
    @combat = Combat.new if armed
  end

  def combat
    @combat
  end

  def tick
    p @combat&.sprites
    p @combat&.update(1)
  end
end

alive = Player.new(true)
alive.tick
alive.tick

dead = Player.new(false)
dead.tick

p alive.combat&.name&.length
p dead.combat&.name&.length
