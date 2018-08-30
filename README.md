# `pbuilder` for Ruby

This branch specifies the deploy workflow for Debian packages of our custom Ruby builds.
To build + deploy:

- Commit your changes to any branch
- Tag your commit as `ruby-shopify-$RUBY_VERSION(-$OPTIONAL_SUFFIX)_$VERSION_NUMBER` (like `ruby-shopify-2.3.1-testing_1`)
- `git push && git push --tags`
- Create a branch to configure packaging `git checkout -b
  shopify/deploy-MY_BUILD debian_pbuilder`
- Commit the package name and push:

```
echo ruby-shopify-${RUBY_VERSION} > NAME_TO_BUILD
git commit NAME_TO_BUILD -m "Building ruby-shopify-${RUBY_VERSION}"
git push --set-upstream origin $(git rev-parse --abbrev-ref HEAD)
```


- A build will be triggered on [Shopify
  Build](https://buildkite.com/shopify/ruby), and the debs uploaded as artifacts
- Download and test your packages, and once happy press the button to
  publish on packagecloud:

<img width="1169" alt="2018-09-03 at 12 08" src="https://user-images.githubusercontent.com/398706/44983732-206dcf00-af72-11e8-8b7b-17de1670360d.png">
