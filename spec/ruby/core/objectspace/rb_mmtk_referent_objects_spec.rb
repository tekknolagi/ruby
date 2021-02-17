require_relative '../../spec_helper'

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
  end

end
