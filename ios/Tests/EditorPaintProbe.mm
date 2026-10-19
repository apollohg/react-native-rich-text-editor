#import "EditorPaintProbe.h"
#import <objc/runtime.h>
#import <QuartzCore/QuartzCore.h>

static const void *PREPEditorPaintProbeKey = &PREPEditorPaintProbeKey;

@implementation PREPEditorPaintProbe {
    __weak NSLayoutManager *_layoutManager;
    Class _originalClass;
}

- (instancetype)initWithLayoutManager:(NSLayoutManager *)layoutManager
{
    if (self = [super init]) {
        _layoutManager = layoutManager;
        _originalClass = object_getClass(layoutManager);
        NSString *name = [@"PREPEditorPaintProbe_" stringByAppendingString:NSStringFromClass(_originalClass)];
        Class observedClass = NSClassFromString(name);
        if (!observedClass) {
            observedClass = objc_allocateClassPair(_originalClass, name.UTF8String, 0);
            SEL selector = @selector(drawGlyphsForGlyphRange:atPoint:);
            Method method = class_getInstanceMethod(_originalClass, selector);
            IMP original = method_getImplementation(method);
            IMP observer = imp_implementationWithBlock(^(NSLayoutManager *manager, NSRange range, CGPoint point) {
                CFTimeInterval started = CACurrentMediaTime();
                ((void (*)(id, SEL, NSRange, CGPoint))original)(manager, selector, range, point);
                CFTimeInterval duration = CACurrentMediaTime() - started;
                PREPEditorPaintProbe *probe = objc_getAssociatedObject(manager, PREPEditorPaintProbeKey);
                if (probe.onDraw) probe.onDraw(range, duration);
            });
            class_addMethod(observedClass, selector, observer, method_getTypeEncoding(method));
            objc_registerClassPair(observedClass);
        }
        objc_setAssociatedObject(layoutManager, PREPEditorPaintProbeKey, self, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        object_setClass(layoutManager, observedClass);
    }
    return self;
}

- (void)close
{
    NSLayoutManager *manager = _layoutManager;
    self.onDraw = nil;
    if (manager) {
        object_setClass(manager, _originalClass);
        objc_setAssociatedObject(manager, PREPEditorPaintProbeKey, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
    _layoutManager = nil;
}

- (NSLayoutManager *)layoutManager { return _layoutManager; }
@end

