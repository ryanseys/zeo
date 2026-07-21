RowA = Struct.new(:id, :name)
RowB = Struct.new(:id, :label)

class RecordCache
  def initialize
    @records = []
    @loaded = false
  end

  def loaded?
    @loaded
  end

  def read_all
    @records
  end

  def replace(records)
    @records = records
    @loaded = true
    @records
  end

  def mark_unloaded
    @loaded = false
  end

  def find_by_id(id)
    index = 0
    while index < @records.length
      record = @records[index]
      return record if record.id == id

      index = index + 1
    end
    nil
  end

  def relationship_exists(id)
    !find_by_id(id).nil?
  end
end

class AStore
  def initialize
    @cache = RecordCache.new
  end

  def load_records
    records = [RowA.new(0, "")]
    records.delete_at(0)
    records << RowA.new(1, "alpha")
    records << RowA.new(2, "next")
    records
  end

  def read_all
    return @cache.read_all if @cache.loaded?

    @cache.replace(load_records)
  end

  def find_by_id(id)
    read_all
    @cache.find_by_id(id)
  end

  def relationship_exists(id)
    read_all
    @cache.relationship_exists(id)
  end
end

class BStore
  def initialize
    @cache = RecordCache.new
  end

  def load_records
    records = [RowB.new(0, "")]
    records.delete_at(0)
    records << RowB.new(1, "first")
    records << RowB.new(2, "beta")
    records
  end

  def read_all
    return @cache.read_all if @cache.loaded?

    @cache.replace(load_records)
  end

  def find_by_id(id)
    read_all
    @cache.find_by_id(id)
  end

  def relationship_exists(id)
    read_all
    @cache.relationship_exists(id)
  end
end

store_a = AStore.new
store_b = BStore.new

puts store_a.read_all[0].name
puts store_b.read_all[1].label
puts store_a.find_by_id(2).name
puts store_b.find_by_id(1).label
puts store_a.relationship_exists(2)
puts store_b.relationship_exists(3)
