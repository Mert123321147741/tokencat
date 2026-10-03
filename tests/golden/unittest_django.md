## ✗ unittest: 3 failed, 21 skipped (652 tests in 0.888s)

### 1. utils_tests.test_text.TestUtilsText.test_slugify (value='__strip__underscore-value___')
AssertionError: '__strip__underscore-value___' != 'strip__underscore-value'
- __strip__underscore-value___
? --                       ---
+ strip__underscore-value
  at /home/dev/django/tests/utils_tests/test_text.py:370 in test_slugify: self.assertEqual(text.slugify(value, allow_unicode=is_unicode), output)

### 2. utils_tests.test_text.TestUtilsText.test_slugify (value='__strip-mixed-value---')
AssertionError: '__strip-mixed-value' != 'strip-mixed-value'
- __strip-mixed-value
? --
+ strip-mixed-value
  at /home/dev/django/tests/utils_tests/test_text.py:370 in test_slugify: self.assertEqual(text.slugify(value, allow_unicode=is_unicode), output)

### 3. utils_tests.test_text.TestUtilsText.test_slugify (value='_ -strip-mixed-value _-')
AssertionError: '_-strip-mixed-value-_' != 'strip-mixed-value'
- _-strip-mixed-value-_
? --                 --
+ strip-mixed-value
  at /home/dev/django/tests/utils_tests/test_text.py:370 in test_slugify: self.assertEqual(text.slugify(value, allow_unicode=is_unicode), output)

[tokencat: 20,693 -> 409 tokens (-98.0%) | saved ~$0.081]
