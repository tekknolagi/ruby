require_relative '../../spec_helper'

require 'set'

module MMTkSpecs

  def self.reaches?(start, to_find)
    worklist = [start]
    seen = Set.new
    until worklist.empty?
      object = worklist.pop
      return true if object.eql?(to_find)
      id = object.object_id
      next if seen.include?(id)
      seen.add id
      worklist.push *ObjectSpace.rb_mmtk_referent_objects(object)
    end
    false
  end

end

describe "ObjectSpace.rb_mmtk_referent_objects" do

  describe "for an Array" do
    it "includes the class" do
      ObjectSpace.rb_mmtk_referent_objects([1, 2, 3]).should include(Array)
    end

    it "includes all elements" do
      referents = ObjectSpace.rb_mmtk_referent_objects([1, 2, 3])
      referents.should include(1)
      referents.should include(2)
      referents.should include(3)
    end

    it "can reach a deeply nested element" do
      MMTkSpecs.reaches?([1, [2, [3], 4], 5], 3).should be_true
    end
  end

end
