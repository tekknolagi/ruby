# `pbuilder` for Ruby

This branch specifies the deploy workflow for Debian packages of our custom Ruby builds.
To deploy:
- Commit your changes to any branch
- Tag your commit as `ruby-shopify-$RUBY-VERSION(-$OPTIONAL_SUFFIX)_$VERSION_NUMBER` (like `ruby-shopify-2.3.1-testing_1`)
- `git push && git push --tags`
- Build your packages at https://shipit.shopify.io/shopify/ruby/production/tasks/build_one/new, specifying the package name and version
- Wait ~10min for the task to complete
- Check https://packages.shopify.io/shopify/public for your packages
